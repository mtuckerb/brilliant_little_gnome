require_relative 'app'
require 'json'

course_id = '446900'

values_path = File.join(File.dirname(__FILE__), 'psy220_values.json')
defs_path = File.join(File.dirname(__FILE__), 'psy220_definitions.json')

if !File.exist?(values_path) || !File.exist?(defs_path)
  puts "Missing data files. Please run the browser scrape first."
  exit 1
end

values = JSON.parse(File.read(values_path))
definitions = JSON.parse(File.read(defs_path))

puts "Syncing PSY-220 grades from local JSON files..."

# We need to mock the client's get_grade_definitions and get_grades 
# because GradeService calls them.
# Or we can just call the sync method directly with our data.

# GradeService.sync(course_id, grade_values)
# Wait, GradeService#sync also calls client.get_grade_definitions internally.
# So we should override that or just manually do the sync logic.

# Let's just use the GradeService but mock the client.
class MockClient
  attr_accessor :values, :definitions
  def initialize(v, d); @values = v; @definitions = d; end
  def get_grade_definitions(cid, force_refresh: false); @definitions; end
  def get_grades(cid, force_refresh: false); @values; end
  def ensure_array(data); data.is_a?(Array) ? data : []; end
end

mock_client = MockClient.new(values, definitions)
grade_service = Brilliant::Sync::GradeService.new(mock_client)

ActiveRecord::Base.connection_pool.with_connection do
  grade_service.sync(course_id, values)
end

puts "Sync complete. Current grades for PSY-220 in DB:"
Grade.where(course_id: course_id).each do |g|
  puts " - #{g.name}: #{g.displayed_grade}"
end
