require_relative 'app'

course = Course.find_by(org_unit_id: '446534')
if course.nil?
  puts "Course LIN 185 (446534) not found!"
  exit 1
end

puts "Current status: #{course.status}"
begin
  if course.update(status: 'early_withdrawal', dropped_at: Time.current)
    puts "Update successful!"
    puts "New status: #{course.status}"
    puts "Dropped at: #{course.dropped_at}"
  else
    puts "Update failed!"
    puts "Errors: #{course.errors.full_messages}"
  end
rescue => e
  puts "Error during update: #{e.message}"
  puts e.backtrace.first(10)
end
