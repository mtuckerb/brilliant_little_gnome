require_relative 'app'

puts "Triggering proactive sync for all courses..."
$client.sync_all_courses_proactively

# The sync runs in a separate thread. We need to wait for it.
puts "Waiting for sync to complete..."
loop do
  status = $client.sync_status
  puts "[#{Time.now.strftime('%H:%M:%S')}] Status: #{status[:status]} - Progress: #{status[:progress]}% - Task: #{status[:current_task]}"
  break if ['completed', 'error'].include?(status[:status])
  sleep 5
end

puts "Final Status: #{$client.sync_status[:status]}"
if $client.sync_status[:status] == 'error'
  puts "Error: #{$client.sync_status[:current_task]}"
end

# Check PSY-220 grades
course_id = '446900'
count = Grade.where(course_id: course_id).count
puts "Number of grades in DB for #{course_id}: #{count}"
Grade.where(course_id: course_id).limit(10).each do |g|
  puts "- #{g.name}: #{g.displayed_grade} (#{g.numerator}/#{g.denominator})"
end
