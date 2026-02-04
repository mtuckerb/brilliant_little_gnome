require_relative 'app'
client = $client
# We need to be authenticated for this to work in a real scenario, 
# but we can try to use the cache if it exists.
course_id = '446562'
course_name = 'SWO 370'
courses = [{ 'OrgUnit' => { 'Id' => course_id, 'Name' => course_name } }]

puts "Fetching content notifications for #{course_name} (ID: #{course_id})..."
updates = client.get_content_notifications(courses)
puts "Found #{updates.size} updates."
updates.each do |u|
  puts "- [#{u[:type]}] #{u[:title]} (ID: #{u[:id]}, Date: #{u[:date]})"
end
