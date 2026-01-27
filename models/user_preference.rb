class UserPreference < ActiveRecord::Base
  serialize :collapsed_topics, type: Array, coder: JSON
  serialize :semester_colors, type: Hash, coder: JSON

  def self.current
    Thread.current[:user_preference] ||= first_or_create!(
      display_name: "User",
      time_zone: "UTC",
      brightspace_host: ENV['BS_HOST'] || "courses.maine.edu",
      collapsed_topics: [],
      historic_gpa: 3.778,
      historic_units: 36,
      api_enabled: false,
      api_listen_all: false,
      remote_server_enabled: false,
      remote_server_ip: nil,
      remote_server_port: 4567,
      show_upcoming_assignments: true,
      show_course_list: true,
      show_recent_updates: true,
      jwt_secret: SecureRandom.hex(32),
      brightspace_uid: nil,
      brightspace_user_id: nil
    )
  end

  def self.get(key)
    pref = self.current rescue nil
    pref && pref.respond_to?(key) ? pref.send(key) : nil
  end

  def self.set(key, value)
    pref = self.current rescue nil
    pref.update(key => value) if pref && pref.respond_to?("#{key}=")
  end

  def topic_collapsed?(topic_id)
    collapsed_topics.include?(topic_id.to_s)
  end

  def toggle_topic_collapse(topic_id)
    tid = topic_id.to_s
    if collapsed_topics.include?(tid)
      collapsed_topics.delete(tid)
    else
      collapsed_topics << tid
    end
    save!
  end
end
