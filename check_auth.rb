require_relative 'app'
puts "Authenticated: #{$client.authenticated?}"
if $client.authenticated?
  puts "User: #{$client.get_who_am_i['DisplayName']}"
else
  puts "Not authenticated. Cookie string length: #{$client.cookie_string&.length || 0}"
end
