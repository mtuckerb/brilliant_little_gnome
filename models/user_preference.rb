class UserPreference < ActiveRecord::Base
  serialize :collapsed_topics, type: Array, coder: JSON

  def self.current
    first_or_create!(
      display_name: "User",
      time_zone: "UTC",
      brightspace_host: ENV['BS_HOST'] || "courses.maine.edu",
      collapsed_topics: [],
      historic_gpa: 3.778,
      historic_units: 36,
      api_enabled: false,
      api_listen_all: false
    )
  end

  def self.get(key)
    pref = self.current
    pref.respond_to?(key) ? pref.send(key) : nil
  end

  def self.set(key, value)
    pref = self.current
    pref.update(key => value) if pref.respond_to?("#{key}=")
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
