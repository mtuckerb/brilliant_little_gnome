require 'ferrum'
require_relative 'app'

course_id = '446900'
client = BrilliantClient.new
host = client.host
cookie_string = client.cookie_string

puts "Scraping PSY-220 grades via Ferrum (headless)..."

browser = Ferrum::Browser.new(timeout: 30)
begin
  # Set cookies
  cookie_string.split(';').each do |pair|
    name, value = pair.split('=', 2).map(&:strip)
    next unless name && value
    browser.cookies.set(name: name, value: value, domain: host, path: '/', secure: true)
  end

  url = "https://#{host}/d2l/lms/grades/my_grades/main.d2l?ou=#{course_id}"
  puts "Navigating to #{url}..."
  browser.goto(url)
  
  # Wait for the table to appear
  browser.network.wait_for_idle
  
  # Check if we are logged in
  if browser.at_css('table[summary="List of grade items and their values"]')
    puts "Found grades table."
  else
    puts "Grades table not found. Cookies might be expired or navigation failed."
    puts "Current URL: #{browser.url}"
    # File.write('debug_page.html', browser.body) # Debug
    exit 1
  end

  # Scrape the data
  rows = browser.evaluate <<-JS
    Array.from(document.querySelectorAll('table[summary="List of grade items and their values"] tr')).map(row => {
      const html = row.innerHTML;
      const cells = Array.from(row.querySelectorAll('th, td')).map(c => c.innerText.trim());
      const match = html.match(/drh\\(\\s*\\d+\\s*,\\s*(\\d+)\\s*\\)/);
      const id = match ? match[1] : null;
      return { id, cells };
    })
  JS

  puts "Scraped #{rows.size} rows."

  ActiveRecord::Base.connection_pool.with_connection do
    rows.each do |row_data|
      id = row_data['id']
      cells = row_data['cells']
      
      # Determine if it's a category or a grade item
      # Usually Grade Items have a specific structure. 
      # In the scraped data, if cells[0] is the name and cells[2] is points, it's a grade item.
      # Wait, the structure was: [Grade Item (0), Points (1), Weight (2), Grade (3), Comments (4)]
      # But some rows have different lengths.
      
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
         # Category?
         name = cells[0]
         points_str = cells[2]
         grade_str = cells[3]
      end

      next if name.nil? || name == "Grade Item" || name.empty?

      # Clean up name (remove (Bonus) etc if we want, but user might want them)
      # Extract numerator/denominator from points_str "90 / 100"
      numerator = nil
      denominator = nil
      if points_str =~ /([\d\.]+)\s*\/\s*([\d\.]+)/
        numerator = $1.to_f
        denominator = $2.to_f
      elsif points_str =~ /\/\s*([\d\.]+)/
        denominator = $1.to_f
      end

      # Generate a synthetic ID if none found
      brightspace_id = id || "scraped_#{Digest::MD5.hexdigest(name)[0..8]}"

      grade = Grade.find_or_initialize_by(course_id: course_id, brightspace_id: brightspace_id)
      grade.name = name
      grade.displayed_grade = grade_str unless grade_str&.empty? || grade_str == "-%"
      grade.numerator = numerator if numerator
      grade.denominator = denominator if denominator
      grade.weight = weight_str.to_f if weight_str =~ /[\d\.]+/
      grade.is_extra_credit = true if name.include?("(Bonus)")
      grade.updated_at = Time.now
      grade.created_at ||= Time.now
      grade.save!
      puts " - Saved: #{name} (#{grade.displayed_grade || 'N/A'}) [ID: #{brightspace_id}]"
    end
  end

  puts "Sync complete."

ensure
  browser.quit
end
