require 'active_record'
require 'json'

# Minimal Model
class Grade < ActiveRecord::Base; end

ActiveRecord::Base.establish_connection(adapter: 'sqlite3', database: 'db/development.sqlite3')

course_id = '446900'
values = JSON.parse(File.read('psy220_values.json'))
definitions = JSON.parse(File.read('psy220_definitions.json'))

puts "Manually inserting grades for #{course_id}..."

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
  
  grade = Grade.find_or_initialize_by(course_id: course_id, brightspace_id: obj_id)
  grade.name = name
  grade.displayed_grade = val&.dig('DisplayedGrade')
  grade.numerator = val&.dig('PointsNumerator') || val&.dig('Numerator')
  grade.denominator = val&.dig('MaxPoints') || val&.dig('PointsDenominator') || val&.dig('Denominator')
  grade.weight = defn&.dig('Weight') || val&.dig('WeightedNumerator')
  grade.is_extra_credit = defn&.dig('IsBonus') || false
  grade.grade_object_type = defn&.dig('GradeType') || val&.dig('GradeObjectTypeName')
  grade.updated_at = Time.now
  grade.created_at ||= Time.now
  grade.save!
  puts " - Saved: #{name} (#{grade.displayed_grade})"
end

puts "Done."
