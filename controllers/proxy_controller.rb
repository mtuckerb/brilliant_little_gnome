require_relative 'base_controller'
require 'digest/sha1'

class ProxyController < BaseController
  # Image Proxy to handle Brightspace Auth and Caching
  get '/api/proxy/banner' do
    url = params[:url]
    halt 400, "URL required" if url.nil? || url.empty?
    url = CGI.unescape(url) if url.include?('%')

    cache_dir = File.expand_path(File.join(settings.public_folder, 'banners'))
    FileUtils.mkdir_p(cache_dir)
    filename = Digest::SHA1.hexdigest(url) + (File.extname(URI.parse(url).path) rescue ".jpg")
    local_path = File.join(cache_dir, filename)

    return send_file local_path if File.exist?(local_path)

    response = $client.download_file(url)
    if response && response.code == '200'
      File.open(local_path, 'wb') { |f| f.write(response.body) }
      content_type response['Content-Type'] || 'image/jpeg'
      response.body
    else
      halt 404, "Image not found"
    end
  end

  # PDF/Download route for topic files
  get '/course/:id/topic/:topic_id/view' do
    api_path = "/d2l/api/le/1.40/#{params[:id]}/content/topics/#{params[:topic_id]}/file"
    http_resp = $client.download_file(api_path)
    
    if http_resp && http_resp.code == '200'
      body_content = http_resp.body
      content_type 'application/pdf'
      response.headers['Content-Disposition'] = "inline; filename=\"topic_#{params[:topic_id]}.pdf\""
      response.headers['Content-Length'] = body_content.to_s.bytesize.to_s
      body_content
    else
      halt (http_resp ? http_resp.code.to_i : 500), "View failed."
    end
  end

  # Generic Download Route
  get '/course/:id/download' do
    path = params[:path]
    path = "/#{path}" unless path.start_with?('/')
    http_resp = $client.download_file(path)
    
    if http_resp && http_resp.code == '200'
      content_type http_resp['Content-Type'] || 'application/octet-stream'
      safe_name = (params[:name] || "download").gsub(/[^0-9A-Za-z.\- ]/, '_')
      headers["Content-Disposition"] = "attachment; filename=\"#{safe_name}\""
      http_resp.body
    else
      halt 500, "Download failed."
    end
  end
end
