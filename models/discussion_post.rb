class DiscussionPost < ActiveRecord::Base
  validates :brightspace_id, presence: true, uniqueness: { scope: [:topic_id, :thread_id] }
  
  def self.sync_from_api(course_id, forum_id, topic_id, api_posts)
    return unless api_posts.is_a?(Array)
    
    transaction do
      api_posts.each do |p|
        post = find_or_initialize_by(
          brightspace_id: p['PostId'].to_s,
          topic_id: topic_id.to_s
        )
        post.thread_id = p['ThreadId'].to_s
        post.parent_post_id = p['ParentPostId'].to_s if p['ParentPostId']
        post.subject = p['Subject']
        post.body = p.dig('Body', 'Html') || p.dig('Body', 'Text')
        post.author_name = p['PostingUserDisplayName']
        post.posted_at = Time.parse(p['DatePosted']) rescue nil
        
        # Determine if instructor based on DisplayName or other metadata
        # (Usually needs to be passed in or checked against a known list)
        post.is_instructor = p.dig('Author', 'IsInstructor') || false
        
        post.save!
      end
    end
  end
end
