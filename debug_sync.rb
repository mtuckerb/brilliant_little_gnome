require_relative 'app'

course_id = '446900'
puts "Starting sync for Course ID: #{course_id}"

# We need to be authenticated. Let's check.
unless $client.authenticated?
  puts "Not authenticated! Please make sure connection.json or cookies.txt is valid."
  exit 1
end

# Try to fetch grades
puts "Fetching grades raw..."
grades_raw = $client.get_grades(course_id, force_refresh: true)
puts "Grades Raw: #{grades_raw.inspect}"

puts "Syncing grades via GradeService..."
$client.sync_grades(course_id, grades_raw)

puts "Check database..."
count = Grade.where(course_id: course_id).count
puts "Number of grades in DB for #{course_id}: #{count}"

Grade.where(course_id: course_id).each do |g|
  puts "- #{g.name}: #{g.displayed_grade} (#{g.numerator}/#{g.denominator})"
end
