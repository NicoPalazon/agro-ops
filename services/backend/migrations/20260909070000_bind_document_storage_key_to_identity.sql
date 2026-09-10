ALTER TABLE documentos
    ADD CONSTRAINT ck_documentos_storage_key_identidad
    CHECK (
        storage_key = 'organizaciones/' || organizacion_id::text
            || '/documentos/' || id::text || '/contenido'
    );

-- Exact identity binding plus the document primary key already makes the key
-- unique; retaining the former standalone unique index would duplicate work.
ALTER TABLE documentos
    DROP CONSTRAINT uq_documentos_storage_key;
