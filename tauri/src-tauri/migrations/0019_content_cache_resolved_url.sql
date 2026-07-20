-- LTI Tools can't have their content cached: the material lives on the vendor's
-- server behind an OAuth-signed launch whose signature expires in minutes. What
-- we CAN capture is where the launch points — the vendor resource URL — which is
-- far more useful than the opaque D2L quicklink it replaces.
ALTER TABLE content_cache ADD COLUMN resolved_url TEXT;
