-- Manual room overrides are user-owned; Brightspace sync never changes them.
ALTER TABLE courses ADD COLUMN custom_room TEXT;
