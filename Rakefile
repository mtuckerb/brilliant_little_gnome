require 'sinatra/activerecord/rake'

# Only load app for DB tasks to avoid version conflicts during platform setup
if Rake.application.top_level_tasks.any? { |t| t.start_with?('db:') }
  require './app'
end

namespace :platforms do
  desc "Lock Gemfile for all supported platforms (macOS arm64/x64, Windows x64)"
  task :lock do
    ruby_path = "bin/ruby_dist/macos-arm64/bin/ruby"
    bundle_path = "bin/ruby_dist/macos-arm64/bin/bundle"
    
    platforms = ["arm64-darwin", "x86_64-darwin", "x64-mingw-ucrt", "ruby"]
    
    puts "Updating lockfile for platforms: #{platforms.join(', ')}..."
    cmd = "#{ruby_path} #{bundle_path} lock --add-platform #{platforms.join(' ')}"
    system(cmd)
  end

  desc "Install gems into vendor/bundle using portable Ruby"
  task :install do
    ruby_path = "bin/ruby_dist/macos-arm64/bin/ruby"
    bundle_path = "bin/ruby_dist/macos-arm64/bin/bundle"
    
    puts "Installing gems into vendor/bundle..."
    # Ensure deployment mode is used for portability
    system("#{ruby_path} #{bundle_path} config set --local path 'vendor/bundle'")
    system("#{ruby_path} #{bundle_path} config set --local deployment 'true'")
    system("#{ruby_path} #{bundle_path} install")
  end
end

