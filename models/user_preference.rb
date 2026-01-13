class UserPreference < ActiveRecord::Base
  def self.current
    first_or_create!(
      display_name: "User",
      time_zone: "UTC",
      brightspace_host: ENV['BS_HOST'] || "courses.maine.edu"
    )
  end
end
