require 'sqlite3'
require 'json'

db = SQLite3::Database.new "db/development.sqlite3"
course_id = '446900'
values = JSON.parse(File.read('psy220_values.json'))
definitions = JSON.parse(File.read('psy220_definitions.json'))

# Map definitions by their ID
definitions_map = definitions.each_with_object({}) do |d, h|
  id = d['Id'].to_s
  h[id] = d
end

# Map values by their grade object ID
values_map = values.each_with_object({}) do |v, h|
  id = (v['GradeObjectIdentifier'] || v['Identifier']).to_s
  h[id] = v
end

all_ids = (definitions_map.keys + values_map.keys).uniq

all_ids.each do |obj_id|
  defn = definitions_map[obj_id]
  val = values_map[obj_id]
  
  name = defn&.dig('Name') || val&.dig('GradeObjectName') || "Grade Item #{obj_id}"
  displayed_grade = val&.dig('DisplayedGrade')
  numerator = val&.dig('PointsNumerator') || val&.dig('Numerator')
  denominator = defn&.dig('MaxPoints') || val&.dig('PointsDenominator') || val&.dig('Denominator')
  weight = defn&.dig('Weight') || val&.dig('WeightedNumerator')
  is_extra_credit = (defn&.dig('IsBonus') ? 1 : 0)
  type = defn&.dig('GradeType') || val&.dig('GradeObjectTypeName')
  now = Time.now.strftime("%Y-%m-%d %H:%M:%S")

  db.execute "INSERT OR REPLACE INTO grades (course_id, brightspace_id, name, displayed_grade, numerator, denominator, weight, is_extra_credit, grade_object_type, updated_at, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
             [course_id, obj_id, name, displayed_grade, numerator, denominator, weight, is_extra_credit, type, now, now]
  puts " - Saved: #{name}"
end
puts "Done."
