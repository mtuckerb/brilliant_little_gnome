require_relative 'base_controller'

class AuthController < BaseController
  get '/setup' do
    @host = $client.host
    erb :setup
  end

  post '/setup' do
    host = params[:host].strip
    cookies = params[:cookies].strip
    $client.save_connection_config(host, cookies)
    $client.degraded_mode = false # Explicitly reset before testing
    whoami = $client.get_who_am_i
    if whoami && whoami['Identifier']
      redirect '/dashboard'
    else
      @error = "Could not authenticate with Brightspace."
      erb :setup
    end
  end

  get '/auth/magic' do
    @host = params[:host] || $client.host || "courses.maine.edu"
    erb :browser_login_prompt
  end

  post '/auth/magic' do
    host = params[:host] || $client.host || "courses.maine.edu"
    # Use the existing helper to capture cookies via a non-headless browser
    cookies = BrilliantAuthHelper.fetch_cookies(host)
    if cookies
      $client.save_connection_config(host, cookies)
      
      # Verify the session before redirecting to ensure we don't end up in a degraded mode loop
      $client.degraded_mode = false
      whoami = $client.get_who_am_i
      if whoami && whoami['Identifier']
        flash[:success] = "Successfully authenticated!"
        redirect '/dashboard'
      else
        @error = "Login captured cookies, but they failed to authenticate with the API. Please try again."
        erb :setup
      end
    else
      @error = "Magic Login failed or timed out."
      erb :setup
    end
  end

  # --- Cookie Transfer Endpoints ---
  # Export current session cookies as JSON (for transferring to headless instances)
  get '/auth/export' do
    content_type :json
    cookie_string = $client.instance_variable_get(:@cookie_string)
    host = $client.host
    if cookie_string && !cookie_string.empty?
      { host: host, cookies: cookie_string, exported_at: Time.now.iso8601 }.to_json
    else
      status 404
      { error: "No active session cookies found. Log in first." }.to_json
    end
  end

  # Import cookies from another instance (JSON body with host + cookies)
  post '/auth/import' do
    begin
      data = JSON.parse(request.body.read)
      host = data['host']&.strip
      cookies = data['cookies']&.strip
      halt 400, { error: "Missing host or cookies" }.to_json unless host && cookies && !cookies.empty?

      $client.save_connection_config(host, cookies)
      $client.degraded_mode = false
      whoami = $client.get_who_am_i
      if whoami && whoami['Identifier']
        content_type :json
        { success: true, user: whoami['Identifier'] }.to_json
      else
        content_type :json
        status 401
        { error: "Cookies saved but failed to authenticate." }.to_json
      end
    rescue JSON::ParserError
      content_type :json
      status 400
      { error: "Invalid JSON body" }.to_json
    end
  end

  get '/callback' do
    if params['code'] && $client.exchange_code(params['code'])
      redirect '/dashboard'
    else
      "Authentication Failed. <a href='/'>Retry</a>"
    end
  end
end
