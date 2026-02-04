require_relative 'app'

course_id = '446900'
client = BrilliantClient.new

puts "Starting sync for PSY-220..."

# Use the client to fetch grades and definitions
# These will be cached in the ApiCache table
grades_raw = client.get_grades(course_id, force_refresh: true)
definitions_raw = client.get_grade_definitions(course_id, force_refresh: true)

if grades_raw.nil? || definitions_raw.nil?
  puts "Failed to fetch data from Brightspace. Check your cookies in config/connection.json"
  exit 1
end

puts "Fetched #{grades_raw.size} grade values and #{definitions_raw.size} definitions."

# Now use GradeService to sync to the Grade table
grade_service = Brilliant::Sync::GradeService.new(client)
ActiveRecord::Base.connection_pool.with_connection do
  grade_service.sync(course_id, grades_raw)
end

puts "Sync complete."
puts "Current PSY-220 Grades in DB:"
Grade.where(course_id: course_id).order(:name).each do |g|
  puts " - [#{g.brightspace_id}] #{g.name}: #{g.displayed_grade || 'Ungraded'}"
end
