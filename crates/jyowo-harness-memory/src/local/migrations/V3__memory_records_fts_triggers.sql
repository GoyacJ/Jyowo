-- V3: Repair memory_records FTS synchronization for databases created before
-- trigger-managed indexing.

CREATE TRIGGER IF NOT EXISTS memory_records_fts_ai
AFTER INSERT ON memory_records
BEGIN
    INSERT INTO memory_records_fts(content, metadata_text, memory_id, tenant_id)
    VALUES (new.content, new.metadata_json, new.id, new.tenant_id);
END;

CREATE TRIGGER IF NOT EXISTS memory_records_fts_au
AFTER UPDATE OF content, metadata_json, id, tenant_id ON memory_records
BEGIN
    DELETE FROM memory_records_fts WHERE memory_id = old.id;
    INSERT INTO memory_records_fts(content, metadata_text, memory_id, tenant_id)
    VALUES (new.content, new.metadata_json, new.id, new.tenant_id);
END;

CREATE TRIGGER IF NOT EXISTS memory_records_fts_ad
AFTER DELETE ON memory_records
BEGIN
    DELETE FROM memory_records_fts WHERE memory_id = old.id;
END;

DELETE FROM memory_records_fts;

INSERT INTO memory_records_fts(content, metadata_text, memory_id, tenant_id)
SELECT content, metadata_json, id, tenant_id
FROM memory_records
WHERE deleted_at IS NULL;
