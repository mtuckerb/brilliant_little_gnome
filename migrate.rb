require_relative 'app'

puts "Running manual migration check..."
context = ActiveRecord::MigrationContext.new("db/migrate")
if context.needs_migration?
  puts "Pending migrations detected. Migrating..."
  context.migrate
  puts "Migrated successfully."
else
  puts "No pending migrations."
end
