require 'ferrum'

class BrilliantAuthHelper
  def self.fetch_cookies(host)
    # Normalize host
    host = host.to_s.gsub(/https?:\/\//, '').split('/').first.strip

    # Launch browser
    # headless: false so the user can actually log in
    browser = Ferrum::Browser.new(
      headless: false,
      window_size: [1024, 768],
      browser_options: { 
        'no-sandbox': nil,
        'user-agent': "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"
      }
    )

    begin
      browser.goto("https://#{host}/d2l/lp/auth/login/login.d2l")

      # We wait until the URL indicates we are logged in.
      # Brightspace usually redirects to /d2l/home/ or /d2l/lp/homepage/
      
      cookies = nil
      
      # Determine base domain for cookie filtering (e.g. maine.edu from courses.maine.edu)
      domain_parts = host.split('.')
      base_domain = domain_parts.size >= 2 ? domain_parts[-2..-1].join('.') : host
      
      # Timeout after 5 minutes
      start_time = Time.now
      loop do
        break if (Time.now - start_time) > 300 # 5 minute timeout
        
        current_url = browser.url
        
        # Get all cookies and filter by domain to avoid sending "foreign" cookies
        # that can cause 431 Request Header Fields Too Large errors.
        all_cookies = browser.cookies.all.values
        relevant_cookies = all_cookies.select { |c| c.domain.include?(base_domain) }
        
        has_session = relevant_cookies.any? { |c| c.name == 'd2lSessionVal' }

        if current_url.include?("/d2l/home") || current_url.include?("/d2l/lp/homepage") || has_session
          # Success! 
          # Wait a few extra seconds to ensure all session-related cookies (like SecureSessionVal) are fully propagated
          sleep 3
          
          # Refresh all_cookies and relevant_cookies one last time
          all_cookies = browser.cookies.all.values
          relevant_cookies = all_cookies.select { |c| c.domain.include?(base_domain) }
          
          # Filter out expired cookies and only keep relevant ones
          valid_cookies = relevant_cookies.reject { |c| c.expires && c.expires < Time.now.to_i }
          
          # Convert to the format BrightspaceClient expects (a single cookie string)
          cookies = valid_cookies.map { |c| "#{c.name}=#{c.value}" }.join("; ")
          break
        end
        
        sleep 1
      end

      cookies
    ensure
      browser.quit
    end
  end
end
