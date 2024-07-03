-- Add down migration script here

ALTER TABLE calendar RENAME COLUMN title_de TO stp_title_de;
ALTER TABLE calendar RENAME COLUMN title_en TO stp_title_en;

DELETE FROM calendar where stp_type is null;
ALTER TABLE calendar ALTER COLUMN stp_type SET NOT NULL;
