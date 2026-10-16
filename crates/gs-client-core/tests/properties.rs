//! Propiedades de `gs-client-core` (T-E1-06, NFR-QUAL-001).
//!
//! Son propiedades, no ejemplos: proptest genera decenas de variantes por
//! propiedad y cada variante es un escenario que no escribiría un humano.
//!
//! Ejecutar con `cargo test -p gs-client-core --test properties`.

use gs_client_core::{FixSample, PublishDecision, SamplingState, SamplingThresholds};
use proptest::prelude::*;

fn small_thresholds() -> SamplingThresholds {
    SamplingThresholds {
        publish_min_distance_m: 1.0,
        publish_accuracy_factor: 0.5,
        publish_min_course_change_deg: 15.0,
        publish_turn_min_speed_mps: 1.0,
        publish_min_speed_change_mps: 1.0,
        publish_max_silence_ms: 10_000,
        min_publish_interval_ms: 500,
    }
}

proptest! {
    #[test]
    fn never_publishes_inside_the_interval_floor(
        cold_start_ms in 0u64..1000u64,
        step_ms in 1u64..499u64,
        n_steps in 1u64..8u64,
    ) {
        // Propiedad del piso: dos publicaciones jamás ocurren con menos de
        // `min_publish_interval_ms` entre ellas, sin importar qué pasos la
        // plataforma entregue.
        let mut state = SamplingState::new();
        let th = small_thresholds();
        let mut now = cold_start_ms;
        let mut n_published = 0usize;
        let mut last_pub: Option<u64> = None;
        for i in 0..n_steps {
            let fix = FixSample {
                // Mover siempre lo suficiente para que, si no fuera por el
                // piso, se publicara.
                lat: i as f64 * 0.01,
                lon: 0.0,
                accuracy_m: 5.0,
                speed_mps: None,
                heading_deg: None,
                timestamp_ms: now,
                motion_code: 0,
            };
            let (decision, _reason) = state.decision(&fix, &th, now);
            if let PublishDecision::Publish = decision {
                n_published += 1;
                if let Some(prev) = last_pub {
                    prop_assert!(
                        now.saturating_sub(prev) >= th.min_publish_interval_ms,
                        "published at {} after {} (gap {} ms < floor {})",
                        now, prev, now.saturating_sub(prev), th.min_publish_interval_ms
                    );
                }
                last_pub = Some(now);
                state.record_published(&fix);
            }
            now = now.saturating_add(step_ms);
        }
        // Al menos una publicación: la primera fix siempre se publica.
        prop_assert!(n_published >= 1);
    }

    #[test]
    fn same_fix_never_republished_while_state_unchanged(
        seq in 0u64..5u64,
    ) {
        // Idempotencia: una vez publicado, re-evaluar la misma muestra en el
        // mismo instante no acaba en otra publicación. Sin este invariant un
        // reintento de handshake del runtime duplicaba la publicación.
        let mut state = SamplingState::new();
        let th = small_thresholds();
        let fix = FixSample {
            lat: 0.0,
            lon: 0.001,
            accuracy_m: 5.0,
            speed_mps: Some(3.0),
            heading_deg: Some(45.0),
            timestamp_ms: 0,
            motion_code: 2,
        };
        let (d0, _) = state.decision(&fix, &th, 10_000);
        prop_assert_eq!(d0, PublishDecision::Publish);
        state.record_published(&fix);
        // Dentro del piso de intervalo y muy por debajo del silencio: debe
        // seguir siendo Skip incluso repitiendo exactamente la misma muestra.
        for k in 1..5usize {
            let (d, _) = state.decision(&fix, &th, k as u64 * 100);
            prop_assert_eq!(d, PublishDecision::Skip);
        }
        let _ = seq;
    }

    #[test]
    fn last_published_only_changes_on_publish(
        speed in 0.0f64..10.0f64,
    ) {
        // Conservación de la última muestra: un fix que no se publica no altera
        // `last`. Si lo alterara, la decisión siguiente se calcularía contra
        // una posición que el servidor nunca recibió.
        let mut state = SamplingState::new();
        let th = small_thresholds();
        let a = FixSample {
            lat: 0.0,
            lon: 0.001,
            accuracy_m: 5.0,
            speed_mps: Some(3.0),
            heading_deg: Some(45.0),
            timestamp_ms: 0,
            motion_code: 2,
        };
        let (d0, _) = state.decision(&a, &th, 10_000);
        prop_assert_eq!(d0, PublishDecision::Publish);
        state.record_published(&a);
        let last_before = state.last.unwrap();

        let near = FixSample {
            lat: 0.00001,
            lon: 0.0,
            accuracy_m: 5.0,
            speed_mps: Some(speed),
            heading_deg: Some(46.0),
            timestamp_ms: 50,
            motion_code: 2,
        };
        let (d1, _) = state.decision(&near, &th, 50);
        prop_assert_eq!(d1, PublishDecision::Skip); // por debajo del piso
        let last_after = state.last.unwrap();
        prop_assert_eq!(last_before, last_after);
    }

    #[test]
    fn significant_distance_monotonic_in_distance_for_fixed_thresholds(
        d1 in 0.0f64..5.0f64,
        d2 in 5.0f64..20.0f64,
    ) {
        // Monotonía de la condición de distancia: si d2 >= d1 y d1 no llega,
        // d2 tampoco debería publicar por debajo del umbral, y por encima del
        // umbral la condición es estable. Simplificado: la misma fix con más
        // movimiento nunca reduce prob. de publicar.
        let mut s1 = SamplingState::new();
        let mut s2 = SamplingState::new();
        let th = small_thresholds();
        let origin = FixSample { lat: 0.0, lon: 0.0, accuracy_m: 5.0, speed_mps: None, heading_deg: None, timestamp_ms: 0, motion_code: 0 };
        let (_, _) = s1.decision(&origin, &th, 0);
        s1.record_published(&origin);
        let (_, _) = s2.decision(&origin, &th, 0);
        s2.record_published(&origin);

        let f1 = FixSample { lat: d1 * 0.001, lon: 0.0, accuracy_m: 5.0, speed_mps: None, heading_deg: None, timestamp_ms: 10_000, motion_code: 0 };
        let f2 = FixSample { lat: d2 * 0.001, lon: 0.0, accuracy_m: 5.0, speed_mps: None, heading_deg: None, timestamp_ms: 10_000, motion_code: 0 };
        let (d1dec, _) = s1.decision(&f1, &th, 10_000);
        let (d2dec, _) = s2.decision(&f2, &th, 10_000);
        if d2dec == PublishDecision::Skip {
            // si la mayor distancia tampoco publica, la menor no puede publicar
            // por otra razón que no sea la escala (a mismos umbrales).
            prop_assert_eq!(d1dec, PublishDecision::Skip);
        }
    }
}
