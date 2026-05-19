-- Override for the Brightspace-assigned semester. Used by the cross-semester
-- drag on Dashboard (opt/alt drop) so a course that Brightspace files under
-- one term can be visually grouped under another. Display logic prefers
-- custom_semester || semester. Brightspace sync never touches it.
ALTER TABLE courses ADD COLUMN custom_semester TEXT;
