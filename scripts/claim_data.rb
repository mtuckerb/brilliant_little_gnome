require_relative '../app'

def claim_orphaned_records
  current_uid = UserPreference.current.brightspace_uid
  unless current_uid
    puts "Error: No brightspace_uid found in UserPreference. Please set it first."
    return
  end

  tables = [
    Course,
    ContentModule,
    ContentItem,
    Assignment,
    Grade,
    DiscussionForum,
    DiscussionTopic,
    DiscussionThread,
    DiscussionPost,
    Notification,
    ApiCache
  ]

  tables.each do |model|
    orphaned = model.where(user_id: nil)
    count = orphaned.count
    if count > 0
      puts "Claiming #{count} orphaned records for #{model.name}..."
      orphaned.update_all(user_id: current_uid)
    else
      puts "No orphaned records for #{model.name}."
    end
  end
end

if __FILE__ == $0
  claim_orphaned_records
end
