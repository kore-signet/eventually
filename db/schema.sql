-- create main table
CREATE TABLE documents (
    doc_id uuid PRIMARY KEY,
    object jsonb
);

CREATE TABLE versions (
  doc_id uuid,
  object jsonb,
  observed bigint,
  hash text
);

-- creates index on documents for faster JSONPATH matching
CREATE INDEX doc_idx ON documents USING gin (object jsonb_path_ops);

-- creates full text search index
CREATE INDEX fts_idx ON documents USING gin (to_tsvector('english', (object ->> 'description'::text)));
