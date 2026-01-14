class UserPreference < ActiveRecord::Base
  serialize :collapsed_topics, Array

  def self.current
    first_or_create!(
      display_name: "User",
      time_zone: "UTC",
      brightspace_host: ENV['BS_HOST'] || "courses.maine.edu",
      collapsed_topics: [],
      historic_gpa: 3.778,
      historic_units: 36
    )
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
