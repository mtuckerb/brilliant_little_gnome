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
      },
      timeout: 120
    )

    begin
      # Clear existing cookies AND storage to ensure we aren't using a stale session
      # that triggers the "already logged in" detection prematurely or causes SSO conflicts.
      browser.cookies.clear
      
      # Clear localStorage and sessionStorage which may contain stale auth state
      # This needs to be done per-origin, so we try for common SSO domains
      browser.goto("about:blank")
      
      browser.evaluate("window.alert = function(){}; window.confirm = function(){return true;}; window.prompt = function(){return null;};")
      
      # Navigate to /d2l/home — redirects through SSO/Google OAuth naturally.
      # Avoid login.d2l — it fires "Invalid Username/Password" alerts on stale/cleared sessions.
      browser.goto("https://#{host}/d2l/home")

      # We wait until the URL indicates we are logged in.
      # Brightspace usually redirects to /d2l/home/ or /d2l/lp/homepage/
      
      cookies = nil
      
      # Determine base domain for cookie filtering (e.g. maine.edu from courses.maine.edu)
      domain_parts = host.split('.')
      base_domain = domain_parts.size >= 2 ? domain_parts[-2..-1].join('.') : host
      
      # Timeout after 5 minutes
      start_time = Time.now
      last_url = ""
      consecutive_checks = 0
      
      loop do
        elapsed = Time.now - start_time
        break if elapsed > 300 # 5 minute timeout
        
        begin
          current_url = browser.url
          
          # Only log if URL changed (for debugging)
          if current_url != last_url
            puts "[Ferrum] URL changed to: #{current_url}"
            last_url = current_url
            consecutive_checks = 0
          else
            consecutive_checks += 1
          end
          
          # Get all cookies and filter by domain to avoid sending "foreign" cookies
          # that can cause 431 Request Header Fields Too Large errors.
          all_cookies = browser.cookies.all.values
          relevant_cookies = all_cookies.select { |c| c.domain.include?(base_domain) }
          
          # d2lSecureSessionVal is the definitive indicator of a logged-in state.
          # d2lSessionVal is often set even before login.
          has_secure_session = relevant_cookies.any? { |c| c.name == 'd2lSecureSessionVal' }

          # Only trust the cookie — URL check alone is unreliable since we
          # navigate TO /d2l/home, so the URL matches before SSO even starts.
          if has_secure_session
            # Success! 
            puts "[Ferrum] Login detected! Waiting for cookie propagation..."
            # Wait a few extra seconds to ensure all session-related cookies (like SecureSessionVal) are fully propagated
            sleep 3
            
            # Refresh all_cookies and relevant_cookies one last time
            all_cookies = browser.cookies.all.values
            relevant_cookies = all_cookies.select { |c| c.domain.include?(base_domain) }
            
            # Filter out expired cookies and only keep relevant ones
            valid_cookies = relevant_cookies.reject { |c| c.expires && c.expires < Time.now.to_i }
            
            # Convert to the format BrightspaceClient expects (a single cookie string)
            cookies = valid_cookies.map { |c| "#{c.name}=#{c.value}" }.join("; ")
            puts "[Ferrum] Captured #{valid_cookies.length} cookies successfully"
            break
          end
          
        rescue Ferrum::TimeoutError => e
          puts "[Ferrum] Navigation timeout (this is normal during OAuth redirects): #{e.message}"
          # Don't break - OAuth redirects can take time. Just continue waiting.
        rescue Ferrum::Error => e
          puts "[Ferrum] Browser error: #{e.message}"
          # Check if we have session cookies even after an error
          begin
            all_cookies = browser.cookies.all.values
            relevant_cookies = all_cookies.select { |c| c.domain.include?(base_domain) }
            if relevant_cookies.any? { |c| c.name == 'd2lSecureSessionVal' }
              puts "[Ferrum] Found session cookie after error, proceeding..."
              cookies = relevant_cookies.map { |c| "#{c.name}=#{c.value}" }.join("; ")
              break
            end
          rescue
            # Continue waiting if we can't check cookies
          end
        end
        
        sleep 1
      end

      if cookies.nil?
        puts "[Ferrum] Login timed out or was cancelled"
      end
      
      cookies
    rescue StandardError => e
      puts "[Ferrum] Unexpected error: #{e.class} - #{e.message}"
      puts e.backtrace.first(5).join("\n")
      nil
    ensure
      begin
        browser.quit
      rescue
        # Ignore errors when closing
      end
    end
  end
end
