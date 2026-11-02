-- 0007_console_passwords.sql — contraseña de la cuenta de consola (ADR-0037, FR-CON-013)
-- Referencia normativa: docs/11_DATA_STORAGE.md §2.0.1, ADR-0037.
-- Aditiva: una columna nueva y anulable. Las cuentas anteriores (sin contraseña) siguen existiendo y
-- fijan una al recuperar el acceso con el código del correo.

-- Solo se guarda el hash PBKDF2-HMAC-SHA256 con su sal y su número de iteraciones
-- (`pbkdf2-sha256$<iteraciones>$<sal>$<hash>`), nunca la contraseña.
ALTER TABLE console_accounts
    ADD COLUMN password_hash   text CHECK (password_hash IS NULL OR char_length(password_hash) BETWEEN 40 AND 300),
    ADD COLUMN password_set_at timestamptz,
    ADD CONSTRAINT console_accounts_password_pair CHECK ((password_hash IS NULL) = (password_set_at IS NULL));
