require_relative 'app'

course_id = '446900'
puts "Fetching grades for course #{course_id}..."

if !$client.authenticated?
  puts "Not authenticated! Cannot fetch grades."
  exit 1
end

grades_raw = $client.get_grades(course_id, force_refresh: true)
puts "Raw grades data type: #{grades_raw.class}"
puts "Raw grades data: #{grades_raw.inspect}"

if grades_raw.is_a?(Array)
  puts "Found #{grades_raw.size} grade values."
  $client.sync_grades(course_id, grades_raw)
  
  count = Grade.where(course_id: course_id).count
  puts "Grades in DB after sync: #{count}"
else
  puts "Grades data is not an array."
end

definitions_raw = $client.get_grade_definitions(course_id, force_refresh: true)
puts "Raw definitions data type: #{definitions_raw.class}"
puts "Found #{definitions_raw.is_a?(Array) ? definitions_raw.size : 'unknown'} definitions."
