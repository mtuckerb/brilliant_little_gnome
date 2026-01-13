require_relative 'lib/brightspace/client'

# Make sure we have the env vars
# Note: In goose, environment variables aren't always shared across shell/ruby unless set.
# But I can assume the client can read them if they are in the environment.

client = BrightspaceClient.new
course_id = "413935"
forum_id = "359745"
topic_id = "492393"

puts "Testing Thread Fetch for Topic #{topic_id}..."
threads = client.get_discussion_threads(course_id, forum_id, topic_id)

puts "Result Type: #{threads.class}"
if threads.is_a?(Hash)
  puts "Keys found: #{threads.keys}"
  items = threads['Items'] || []
  puts "Items count: #{items.size}"
elsif threads.is_a?(Array)
  puts "Items count: #{threads.size}"
else
  puts "Result: #{threads.inspect}"
end
