class DiscussionPost < ActiveRecord::Base
  validates :brightspace_id, presence: true, uniqueness: { scope: [:topic_id, :thread_id] }
  
  belongs_to :discussion_topic, foreign_key: :topic_id, primary_key: :brightspace_id

  after_save :update_topic_counts
  after_destroy :update_topic_counts

  def update_topic_counts
    return unless discussion_topic
    
    # Use update_columns to avoid triggering callbacks on the topic itself
    # and to be as efficient as possible.
    discussion_topic.update_columns(
      thread_count: discussion_topic.discussion_posts.distinct.count(:thread_id),
      post_count: discussion_topic.discussion_posts.count,
      last_post_date: discussion_topic.discussion_posts.maximum(:posted_at)
    )
  end

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
        post.is_instructor = p.dig('Author', 'IsInstructor') == true || p.dig('Author', 'RoleName') =~ /Instructor/i rescue false
        
        post.save!
      end
    end
  end
end
