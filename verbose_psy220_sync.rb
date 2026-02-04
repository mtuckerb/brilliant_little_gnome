require_relative 'app'

course_id = '446900'
puts "Starting verbose sync for PSY-220 (ID: #{course_id})..."

if !$client.authenticated?
  puts "Authentication check failed. Cookie length: #{$client.cookie_string&.length}"
  # Try to reload config just in case
  $client.load_connection_config
  puts "After reload, authenticated: #{$client.authenticated?}"
end

if !$client.authenticated?
  puts "CRITICAL: Not authenticated. Cannot proceed."
  exit 1
end

puts "1. Fetching Enrollment info..."
enrollments = $client.get_enrollments(force_refresh: true)
psy_course = enrollments.find { |e| e['OrgUnit']['Id'].to_s == course_id }
if psy_course
  puts "Found course: #{psy_course['OrgUnit']['Name']}"
else
  puts "Course #{course_id} NOT found in enrollments!"
  puts "Available courses: #{enrollments.map{|e| e['OrgUnit']['Id']}.join(', ')}"
end

puts "\n2. Fetching Grade Definitions..."
path_defs = "/d2l/api/le/1.40/#{course_id}/grades/"
defs = $client.do_get(path_defs, force_refresh: true)
puts "Definitions response: #{defs.class}"
if defs.is_a?(Array)
  puts "Found #{defs.size} definitions."
  defs.each do |d|
    puts " - [#{d['Identifier']}] #{d['Name']} (Type: #{d['GradeType']})"
  end
elsif defs.is_a?(Hash) && defs['Objects']
  puts "Found #{defs['Objects'].size} definitions in Objects."
else
  puts "Definitions response was unexpected: #{defs.inspect}"
end

puts "\n3. Fetching Grade Values..."
path_vals = "/d2l/api/le/1.40/#{course_id}/grades/values/myGradeValues/"
vals = $client.do_get(path_vals, force_refresh: true)
puts "Values response: #{vals.class}"
if vals.is_a?(Array)
  puts "Found #{vals.size} values."
  vals.each do |v|
     id = v['GradeObjectIdentifier'] || v['Identifier']
     puts " - [#{id}] #{v['GradeObjectName']}: #{v['DisplayedGrade']}"
  end
else
  puts "Values response was unexpected: #{vals.inspect}"
end

puts "\n4. Running Grade Sync..."
$client.sync_grades(course_id, vals)

puts "\n5. Final DB check..."
count = Grade.where(course_id: course_id).count
puts "Total grades in DB for #{course_id}: #{count}"
Grade.where(course_id: course_id).each do |g|
  puts " - #{g.name}: #{g.displayed_grade} (Ungraded: #{g.manually_marked_ungraded})"
end
