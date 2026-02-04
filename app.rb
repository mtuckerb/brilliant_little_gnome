require 'bundler/setup'
require 'fileutils'
require 'uri'
require 'cgi'
require 'securerandom'

# Handle --headless flag before Sinatra/Bundler parses ARGV
$headless_mode = ARGV.delete('--headless')

# Stream handling for sidecar stability
def $stderr.write(data); super rescue nil; end
def $stdout.write(data); super rescue nil; end

Bundler.require(:default)

require 'sinatra/base'
require 'sinatra/activerecord'
require 'active_support/time'
require 'rack-flash'
require 'jwt'

# --- Load Models & Helpers ---
require_relative 'models/concerns/has_user_identity'
Dir.glob('./models/*.rb').each { |f| require_relative f }
require_relative 'helpers/course_helpers'
require_relative 'lib/brilliant/text_processor'
require_relative 'lib/brilliant/dashboard_service'
require_relative 'lib/brilliant/event_bus'
require_relative 'lib/brilliant/sync/base_service'
Dir.glob('./lib/brilliant/sync/*.rb').each { |f| require_relative f }
require_relative 'lib/brilliant/client'
require_relative 'lib/brilliant/auth'
require_relative 'lib/brilliant/auth_helper'

# --- Initialization ---
begin
  # Use DATABASE_URL or default to SQLite
  db_url = ENV['DATABASE_URL'] || "sqlite3://#{File.expand_path('db/development.sqlite3')}"
  if db_url.start_with?('sqlite3://')
    db_path = CGI.unescape(URI.parse(db_url).path)
    FileUtils.mkdir_p(File.dirname(db_path))
    ActiveRecord::Base.establish_connection(adapter: 'sqlite3', database: db_path, pool: 20, timeout: 5000)
  else
    ActiveRecord::Base.establish_connection(db_url)
  end
  # Timezone configuration (handle different ActiveRecord versions)
  if ActiveRecord.respond_to?(:default_timezone=)
    ActiveRecord.default_timezone = :utc
  elsif ActiveRecord::Base.respond_to?(:default_timezone=)
    ActiveRecord::Base.default_timezone = :utc
  end
  
  # WAL mode & migrations
  ActiveRecord::Base.connection.execute("PRAGMA journal_mode=WAL;") rescue nil
  ActiveRecord::MigrationContext.new("db/migrate").migrate if ActiveRecord::MigrationContext.new("db/migrate").needs_migration?
  
  # Ensure models have latest column info after migrations
  UserPreference.reset_column_information if defined?(UserPreference)
  Course.reset_column_information if defined?(Course)
  Assignment.reset_column_information if defined?(Assignment)
  Grade.reset_column_information if defined?(Grade)
  Notification.reset_column_information if defined?(Notification)
rescue => e
  puts "[Brilliant] DB Init Error: #{e.message}"
end

$client = BrilliantClient.new

# --- Load Controllers ---
require_relative 'controllers/base_controller'
require_relative 'controllers/auth_controller'
require_relative 'controllers/dashboard_controller'
require_relative 'controllers/course_controller'
require_relative 'controllers/assignment_controller'
require_relative 'controllers/discussion_controller'
require_relative 'controllers/proxy_controller'
require_relative 'controllers/sync_controller'
require_relative 'controllers/mcp/mcp_controller'
require_relative 'controllers/api/v1/api_controller'

class BrilliantApp < BaseController
  # Global settings
  set :port, 4567
  set :bind, '0.0.0.0'

  # Use explicit session middleware at the top of the stack to ensure all modular
  # controllers (used as middleware) can access the session.
  use Rack::Session::Cookie,
      key: 'brilliant.session',
      path: '/',
      expire_after: 2592000, # 30 days
      secret: (ENV['SESSION_SECRET'] || 'brilliant_app_session_persistent_secret_12345'),
      same_site: :lax,
      secure: false

  use Rack::Flash, :sweep => true
  use Rack::CommonLogger, $stdout

  # --- Middleware / Middleware-style controllers ---
  use AuthController
  use DashboardController
  use CourseController
  use AssignmentController
  use DiscussionController
  use ProxyController
  use SyncController
  use McpController
  use Api::V1::ApiController

  # PID Management
  if ENV['BRILLIANT_DATA_DIR']
    pid_path = File.join(ENV['BRILLIANT_DATA_DIR'], 'ruby_sidecar.pid')
    File.write(pid_path, Process.pid.to_s)
    at_exit { File.delete(pid_path) if File.exist?(pid_path) }
  end

  # --- Error Handling ---
  not_found do
    @error_title = "404 - Not Found"
    @error_message = "The page you are looking for does not exist."
    erb :error
  end

  error do
    @error = env['sinatra.error']
    @error_title = "500 - Server Error"
    @error_message = "An unexpected error occurred."
    puts "[Brilliant Error] #{@error.message}" if @error
    puts @error.backtrace.first(10).join("\n") if @error && @error.respond_to?(:backtrace)
    erb :error
  end

  get '/*' do
    puts "[Brilliant 404 Catch-All] #{request.request_method} #{request.path_info} - Params: #{params.inspect}"
    pass # Let it fall through to the not_found block
  end
end

if __FILE__ == $0
  BrilliantApp.run!
end
