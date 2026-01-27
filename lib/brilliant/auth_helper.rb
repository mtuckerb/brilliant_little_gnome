require 'ferrum'

class BrilliantAuthHelper
  def self.fetch_cookies(host)
    # Launch browser
    # headless: false so the user can actually log in
    browser = Ferrum::Browser.new(
      headless: false,
      window_size: [1024, 768],
      browser_options: { 'no-sandbox': nil }
    )

    begin
      browser.goto("https://#{host}/d2l/lp/auth/login/login.d2l")

      # We wait until the URL indicates we are logged in.
      # Brightspace usually redirects to /d2l/home/ or /d2l/lp/homepage/
      
      cookies = nil
      
      # Timeout after 5 minutes
      start_time = Time.now
      loop do
        break if (Time.now - start_time) > 300 # 5 minute timeout
        
        current_url = browser.url
        has_session = browser.cookies.all.values.any? { |c| c.name == 'd2lSessionVal' }

        if current_url.include?("/d2l/home") || current_url.include?("/d2l/lp/homepage") || has_session
          # Success! Grab all cookies
          # Ferrum returns an array of cookie hashes
          raw_cookies = browser.cookies.all.values
          
          # Convert to the format BrightspaceClient expects (a single cookie string)
          # Or we can store them as JSON if we want to be fancy, but the client expects a string.
          cookies = raw_cookies.map { |c| "#{c.name}=#{c.value}" }.join("; ")
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
