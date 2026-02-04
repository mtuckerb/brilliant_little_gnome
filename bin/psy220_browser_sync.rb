require 'ferrum'
require 'sqlite3'
require 'json'
require 'digest'
require 'fileutils'

# We don't load 'app.rb' to avoid dependency hell in cron
# We just read the config and update the DB directly

BASE_DIR = File.expand_path('..', __dir__)
# Prefer Application Support for production data if it exists
APP_SUPPORT = File.expand_path("~/Library/Application Support/Brilliant")
if Dir.exist?(APP_SUPPORT)
  CONFIG_PATH = File.join(APP_SUPPORT, 'config', 'connection.json')
  DB_PATH = File.join(APP_SUPPORT, 'db', 'production.sqlite3')
  LOG_PATH = File.join(APP_SUPPORT, 'logs', 'psy220_sync.log')
else
  CONFIG_PATH = File.join(BASE_DIR, 'config', 'connection.json')
  DB_PATH = File.join(BASE_DIR, 'db', 'development.sqlite3')
  LOG_PATH = File.join(BASE_DIR, 'logs', 'psy220_sync.log')
end

FileUtils.mkdir_p(File.dirname(LOG_PATH))

def log(msg)
  tms = Time.now.strftime('%Y-%m-%d %H:%M:%S')
  puts "[#{tms}] #{msg}"
  File.open(LOG_PATH, 'a') { |f| f.puts "[#{tms}] #{msg}" }
end

log "Starting PSY-220 Browser Sync..."

unless File.exist?(CONFIG_PATH)
  log "Error: Config not found at #{CONFIG_PATH}"
  exit 1
end

begin
  config = JSON.parse(File.read(CONFIG_PATH))
rescue => e
  log "Error: Failed to parse config: #{e.message}"
  exit 1
end

host = config['host'] || "courses.maine.edu"
cookie_string = config['cookies']

if !cookie_string || cookie_string.empty?
  log "Error: No cookies found in config."
  exit 1
end

course_id = '446900'
browser = Ferrum::Browser.new(timeout: 60, headless: true)

begin
  # Set cookies
  cookie_string.split(';').each do |pair|
    name, value = pair.split('=', 2).map(&:strip)
    next unless name && value
    browser.cookies.set(name: name, value: value, domain: host, path: '/', secure: true)
  end

  url = "https://#{host}/d2l/lms/grades/my_grades/main.d2l?ou=#{course_id}"
  log "Navigating to #{url}..."
  browser.goto(url)
  
  # Wait for table
  browser.network.wait_for_idle
  
  unless browser.at_css('table[summary="List of grade items and their values"]')
    log "Error: Grades table not found. Cookies might be expired."
    exit 1
  end

  # Scrape
  rows = browser.evaluate <<-JS
    Array.from(document.querySelectorAll('table[summary="List of grade items and their values"] tr')).map(row => {
      const html = row.innerHTML;
      const cells = Array.from(row.querySelectorAll('th, td')).map(c => c.innerText.trim());
      const match = html.match(/drh\\(\\s*\\d+\\s*,\\s*(\\d+)\\s*\\)/);
      const id = match ? match[1] : null;
      return { id, cells };
    })
  JS

  log "Scraped #{rows.size} rows."

  db = SQLite3::Database.new(DB_PATH)
  now = Time.now.strftime('%Y-%m-%d %H:%M:%S')

  count = 0
  rows.each do |row_data|
    id = row_data['id']
    cells = row_data['cells']
    
    name = nil
    points_str = nil
    grade_str = nil
    weight_str = nil

    if cells.length >= 5
      name = cells[0].empty? ? cells[1] : cells[0]
      points_str = cells[2]
      weight_str = cells[3]
      grade_str = cells[4]
    elsif cells.length == 4
       name = cells[0]
       points_str = cells[2]
       grade_str = cells[3]
    end

    next if name.nil? || name == "Grade Item" || name.empty?

    # Extract numerator/denominator
    numerator = nil
    denominator = nil
    if points_str =~ /([\d\.]+)\s*\/\s*([\d\.]+)/
      numerator = $1.to_f
      denominator = $2.to_f
    elsif points_str =~ /\/\s*([\d\.]+)/
      denominator = $1.to_f
    end

    # Handle "Dropped!" status
    if points_str.include?("Dropped!") && numerator.nil?
       if points_str =~ /^([\d\.]+)/
         numerator = $1.to_f
       end
    end

    displayed = (grade_str == "-%" || grade_str&.empty?) ? nil : grade_str
    
    # Try to find existing ID by name if not provided
    brightspace_id = id || db.get_first_value("SELECT brightspace_id FROM grades WHERE course_id = ? AND name = ?", [course_id, name])
    brightspace_id ||= "scraped_#{Digest::MD5.hexdigest(name)[0..8]}"

    db.execute(<<-SQL, [course_id, brightspace_id, name, displayed, numerator, denominator, now, now])
      INSERT INTO grades (course_id, brightspace_id, name, displayed_grade, numerator, denominator, updated_at, created_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?)
      ON CONFLICT(course_id, brightspace_id) DO UPDATE SET
        name = excluded.name,
        displayed_grade = excluded.displayed_grade,
        numerator = COALESCE(excluded.numerator, numerator),
        denominator = COALESCE(excluded.denominator, denominator),
        updated_at = excluded.updated_at
SQL
    count += 1
  end

  db.close
  log "Sync complete. Upserted #{count} items."

rescue => e
  log "Error during sync: #{e.message}"
  log e.backtrace.join("\n")
ensure
  browser.quit rescue nil
end
