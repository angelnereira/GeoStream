//! Device repository with strict tenant isolation and device limits (docs/11 §2, docs/05 §3.4, INV-TEN-1, INV-DEV-1, INV-DEV-2).
#![forbid(unsafe_code)]

use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use gs_domain::ids::{ApplicationId, DeviceId, InstallationId, SubjectId, TenantId};

use crate::error::StoreError;
use crate::models::{Device, DevicePlatform, DeviceStatus};

pub struct DeviceRepository;

/// Parameters for [`DeviceRepository::upsert_device`] (FR-AUTH-002, INV-DEV-1, INV-DEV-2).
#[derive(Debug, Clone)]
pub struct UpsertDeviceParams {
    pub tenant_id: TenantId,
    pub subject_id: SubjectId,
    pub application_id: ApplicationId,
    pub installation_id: InstallationId,
    pub platform: DevicePlatform,
    pub app_version: Option<String>,
    pub sdk_version: Option<String>,
    pub max_devices_per_subject: u32,
    pub now: OffsetDateTime,
}

impl DeviceRepository {
    /// Inserts a new device record.
    pub async fn create(pool: &PgPool, device: &Device) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO devices (
                id, tenant_id, subject_id, application_id, installation_id,
                platform, app_version, sdk_version, status,
                created_at, last_seen_at, deactivated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12);
            "#,
        )
        .bind(device.id.into_uuid())
        .bind(device.tenant_id.into_uuid())
        .bind(device.subject_id.into_uuid())
        .bind(device.application_id.into_uuid())
        .bind(device.installation_id.into_uuid())
        .bind(device.platform.as_str())
        .bind(&device.app_version)
        .bind(&device.sdk_version)
        .bind(device.status.as_str())
        .bind(device.created_at)
        .bind(device.last_seen_at)
        .bind(device.deactivated_at)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(())
    }

    /// Fetches a device by identifier, scoped to a specific tenant.
    pub async fn get_by_id(
        pool: &PgPool,
        tenant_id: TenantId,
        device_id: DeviceId,
    ) -> Result<Option<Device>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, subject_id, application_id, installation_id,
                   platform, app_version, sdk_version, status,
                   created_at, last_seen_at, deactivated_at
            FROM devices
            WHERE id = $1 AND tenant_id = $2;
            "#,
        )
        .bind(device_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.map(Self::map_row).transpose()
    }

    /// Finds an active device by its identity tuple (application_id, installation_id, subject_id).
    pub async fn find_active_by_identity(
        pool: &PgPool,
        app_id: ApplicationId,
        installation_id: InstallationId,
        subject_id: SubjectId,
    ) -> Result<Option<Device>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, subject_id, application_id, installation_id,
                   platform, app_version, sdk_version, status,
                   created_at, last_seen_at, deactivated_at
            FROM devices
            WHERE application_id = $1 AND installation_id = $2 AND subject_id = $3 AND status = 'ACTIVE';
            "#,
        )
        .bind(app_id.into_uuid())
        .bind(installation_id.into_uuid())
        .bind(subject_id.into_uuid())
        .fetch_optional(pool)
        .await
        .map_err(StoreError::Database)?;

        row.map(Self::map_row).transpose()
    }

    /// Updates `last_seen_at` and optional versions for an active device.
    pub async fn update_last_seen(
        pool: &PgPool,
        tenant_id: TenantId,
        device_id: DeviceId,
        app_version: Option<&str>,
        sdk_version: Option<&str>,
        now: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let res = sqlx::query(
            r#"
            UPDATE devices
            SET last_seen_at = $3,
                app_version = COALESCE($4, app_version),
                sdk_version = COALESCE($5, sdk_version)
            WHERE id = $1 AND tenant_id = $2 AND status = 'ACTIVE';
            "#,
        )
        .bind(device_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(now)
        .bind(app_version)
        .bind(sdk_version)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(res.rows_affected() > 0)
    }

    /// Deactivates a device, setting status to INACTIVE and recording deactivated_at.
    pub async fn deactivate(
        pool: &PgPool,
        tenant_id: TenantId,
        device_id: DeviceId,
        now: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let res = sqlx::query(
            r#"
            UPDATE devices
            SET status = 'INACTIVE', deactivated_at = $3
            WHERE id = $1 AND tenant_id = $2 AND status = 'ACTIVE';
            "#,
        )
        .bind(device_id.into_uuid())
        .bind(tenant_id.into_uuid())
        .bind(now)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(res.rows_affected() > 0)
    }

    /// Lists active devices for a subject, ordered by most recently seen first.
    pub async fn list_active_by_subject(
        pool: &PgPool,
        tenant_id: TenantId,
        subject_id: SubjectId,
    ) -> Result<Vec<Device>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, subject_id, application_id, installation_id,
                   platform, app_version, sdk_version, status,
                   created_at, last_seen_at, deactivated_at
            FROM devices
            WHERE tenant_id = $1 AND subject_id = $2 AND status = 'ACTIVE'
            ORDER BY last_seen_at DESC;
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(subject_id.into_uuid())
        .fetch_all(pool)
        .await
        .map_err(StoreError::Database)?;

        rows.into_iter().map(Self::map_row).collect()
    }

    /// Enforces `max_devices_per_subject` (INV-DEV-2).
    /// If active devices exceed `max_devices`, the oldest ones by `last_seen_at` are marked INACTIVE.
    /// Returns the number of devices deactivated.
    pub async fn enforce_max_devices(
        pool: &PgPool,
        tenant_id: TenantId,
        subject_id: SubjectId,
        max_devices: u32,
        now: OffsetDateTime,
    ) -> Result<u64, StoreError> {
        let res = sqlx::query(
            r#"
            WITH active_devices AS (
                SELECT id, ROW_NUMBER() OVER (ORDER BY last_seen_at DESC, id DESC) AS rn
                FROM devices
                WHERE tenant_id = $1 AND subject_id = $2 AND status = 'ACTIVE'
            ),
            excess AS (
                SELECT id FROM active_devices WHERE rn > $3
            )
            UPDATE devices
            SET status = 'INACTIVE', deactivated_at = $4
            WHERE id IN (SELECT id FROM excess);
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(subject_id.into_uuid())
        .bind(max_devices as i64)
        .bind(now)
        .execute(pool)
        .await
        .map_err(StoreError::Database)?;

        Ok(res.rows_affected())
    }

    /// Upserts a device for token minting (FR-AUTH-002, INV-DEV-1, INV-DEV-2).
    /// If an active device exists for `(application_id, installation_id, subject_id)`, updates it.
    /// Otherwise, creates a new active device and enforces `max_devices`.
    pub async fn upsert_device(
        pool: &PgPool,
        params: UpsertDeviceParams,
    ) -> Result<Device, StoreError> {
        let UpsertDeviceParams {
            tenant_id,
            subject_id,
            application_id,
            installation_id,
            platform,
            app_version,
            sdk_version,
            max_devices_per_subject,
            now,
        } = params;

        if let Some(existing) =
            Self::find_active_by_identity(pool, application_id, installation_id, subject_id).await?
        {
            let updated_row = sqlx::query(
                r#"
                UPDATE devices
                SET platform = $2,
                    app_version = COALESCE($3, app_version),
                    sdk_version = COALESCE($4, sdk_version),
                    last_seen_at = $5
                WHERE id = $1
                RETURNING id, tenant_id, subject_id, application_id, installation_id,
                          platform, app_version, sdk_version, status,
                          created_at, last_seen_at, deactivated_at;
                "#,
            )
            .bind(existing.id.into_uuid())
            .bind(platform.as_str())
            .bind(&app_version)
            .bind(&sdk_version)
            .bind(now)
            .fetch_one(pool)
            .await
            .map_err(StoreError::Database)?;

            Self::map_row(updated_row)
        } else {
            let new_device = Device {
                id: DeviceId::new(),
                tenant_id,
                subject_id,
                application_id,
                installation_id,
                platform,
                app_version,
                sdk_version,
                status: DeviceStatus::Active,
                created_at: now,
                last_seen_at: now,
                deactivated_at: None,
            };

            Self::create(pool, &new_device).await?;
            Self::enforce_max_devices(pool, tenant_id, subject_id, max_devices_per_subject, now)
                .await?;
            Ok(new_device)
        }
    }

    fn map_row(row: sqlx::postgres::PgRow) -> Result<Device, StoreError> {
        let id_raw: Uuid = row.get("id");
        let tenant_id_raw: Uuid = row.get("tenant_id");
        let subject_id_raw: Uuid = row.get("subject_id");
        let app_id_raw: Uuid = row.get("application_id");
        let inst_raw: Uuid = row.get("installation_id");

        let installation_id = InstallationId::from_uuid(inst_raw)
            .map_err(|e| StoreError::DataMapping(format!("invalid installation_id in db: {e}")))?;

        let platform_str: String = row.get("platform");
        let platform = platform_str.parse().map_err(|_| {
            StoreError::DataMapping(format!("invalid device platform: {platform_str}"))
        })?;

        let status_str: String = row.get("status");
        let status = status_str
            .parse()
            .map_err(|_| StoreError::DataMapping(format!("invalid device status: {status_str}")))?;

        Ok(Device {
            id: DeviceId::from_uuid(id_raw),
            tenant_id: TenantId::from_uuid(tenant_id_raw),
            subject_id: SubjectId::from_uuid(subject_id_raw),
            application_id: ApplicationId::from_uuid(app_id_raw),
            installation_id,
            platform,
            app_version: row.get("app_version"),
            sdk_version: row.get("sdk_version"),
            status,
            created_at: row.get("created_at"),
            last_seen_at: row.get("last_seen_at"),
            deactivated_at: row.get("deactivated_at"),
        })
    }
}
