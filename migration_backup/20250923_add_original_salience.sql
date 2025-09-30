-- Migration to add original_salience column for decay system
-- Create this file as: migrations/20250923_add_original_salience.sql

-- Add the missing original_salience column
ALTER TABLE message_analysis ADD COLUMN original_salience REAL;

-- Populate original_salience with current salience values for existing records
-- This assumes current salience values are the original ones (reasonable for migration)
UPDATE message_analysis 
SET original_salience = salience 
WHERE original_salience IS NULL AND salience IS NOT NULL;

-- Add index for decay queries
CREATE INDEX idx_analysis_original_salience ON message_analysis(original_salience);

-- Update schema metadata
INSERT INTO schema_metadata (version, description) 
VALUES ('1.0.1', 'Added original_salience column for memory decay system');
