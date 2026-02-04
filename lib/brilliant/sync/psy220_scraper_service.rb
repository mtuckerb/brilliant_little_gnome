require 'ferrum'

module Brilliant
  module Sync
    class Psy220ScraperService < BaseService
      def sync(course_id)
        return unless course_id.to_s == '446900'
        
        host = client.host
        cookie_string = client.cookie_string
        
        puts "[Psy220Scraper] Starting scrape for PSY-220..."
        
        # Use a longer timeout for Ferrum
        browser = Ferrum::Browser.new(timeout: 60, browser_options: { 'no-sandbox': nil })
        begin
          # Set cookies
          cookie_string.split(';').each do |pair|
            name, value = pair.split('=', 2).map(&:strip)
            next unless name && value
            browser.cookies.set(name: name, value: value, domain: host, path: '/', secure: true)
          end

          url = "https://#{host}/d2l/lms/grades/my_grades/main.d2l?ou=#{course_id}"
          browser.goto(url)
          
          # Wait for the table to appear
          browser.network.wait_for_idle
          
          # Check if we are logged in
          unless browser.at_css('table[summary="List of grade items and their values"]')
            puts "[Psy220Scraper] Grades table not found. Cookies might be expired."
            return false
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

          puts "[Psy220Scraper] Scraped #{rows.size} rows."

          ActiveRecord::Base.connection_pool.with_connection do
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

              numerator = nil
              denominator = nil
              if points_str =~ /([\d\.]+)\s*\/\s*([\d\.]+)/
                numerator = $1.to_f
                denominator = $2.to_f
              elsif points_str =~ /\/\s*([\d\.]+)/
                denominator = $1.to_f
              end

              brightspace_id = id || "scraped_#{Digest::MD5.hexdigest(name)[0..8]}"

              grade = Grade.find_or_initialize_by(course_id: course_id.to_s, brightspace_id: brightspace_id)
              grade.name = name
              
              # Only update if we have a real value or if it's currently nil
              unless grade_str&.empty? || grade_str == "-%"
                grade.displayed_grade = grade_str
              end
              
              grade.numerator = numerator if numerator
              grade.denominator = denominator if denominator
              grade.weight = weight_str.to_f if weight_str =~ /[\d\.]+/
              grade.is_extra_credit = true if name.include?("(Bonus)")
              grade.updated_at = Time.current
              grade.save!
            end
          end

          puts "[Psy220Scraper] Sync complete."
          return true

        rescue => e
          puts "[Psy220Scraper] Failed: #{e.message}"
          return false
        ensure
          browser.quit rescue nil
        end
      end
    end
  end
end
