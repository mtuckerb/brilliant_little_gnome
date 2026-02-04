
require_relative 'config/environment'
client = BrilliantClient.new
courses = client.get_enrollments
puts "Total enrollments: #{courses.size}"
courses.each_with_index do |c, i|
  puts "#{i+1}: #{c['OrgUnit']['Name']} (ID: #{c['OrgUnit']['Id']})"
end
