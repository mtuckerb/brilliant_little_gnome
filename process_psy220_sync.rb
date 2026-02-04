require_relative 'app'
require 'json'

course_id = '446900'
definitions = JSON.parse(File.read('psy220_definitions.json'))
values = JSON.parse(File.read('psy220_values.json'))

puts "Processing #{definitions.size} definitions and #{values.size} values for PSY-220..."

# Use the same logic as GradeService
service = Brilliant::Sync::GradeService.new($client)

# We need to mock the client's get_grade_definitions to return our local data
class << $client
  attr_accessor :mock_definitions
  def get_grade_definitions(id, **opts)
    @mock_definitions
  end
end
$client.mock_definitions = definitions

service.sync(course_id, values)

puts "Sync complete."
count = Grade.where(course_id: course_id).count
puts "Number of grades in DB for #{course_id}: #{count}"
Grade.where(course_id: course_id).each do |g|
  puts "- #{g.name}: #{g.displayed_grade} (#{g.numerator}/#{g.denominator})"
end
