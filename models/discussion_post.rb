class DiscussionPost < ActiveRecord::Base
  include HasUserIdentity
  validates :brightspace_id, presence: true, uniqueness: { scope: [:topic_id, :thread_id] }
  
  belongs_to :discussion_topic, foreign_key: :topic_id, primary_key: :brightspace_id

  after_save :update_topic_counts, unless: -> { Thread.current[:skip_discussion_callbacks] }
  after_destroy :update_topic_counts, unless: -> { Thread.current[:skip_discussion_callbacks] }

  def update_topic_counts
    return unless discussion_topic
    
    # Use update_columns to avoid triggering callbacks on the topic itself
    # and to be as efficient as possible.
    discussion_topic.update_columns(
      thread_count: DiscussionPost.where(topic_id: topic_id.to_s).distinct.count(:thread_id),
      post_count: DiscussionPost.where(topic_id: topic_id.to_s).count,
      last_post_date: DiscussionPost.where(topic_id: topic_id.to_s).maximum(:posted_at)
    )
  end

  def self.sync_from_api(course_id, forum_id, topic_id, api_posts)
    return unless api_posts.is_a?(Array) && api_posts.any?
    
    Thread.current[:skip_discussion_callbacks] = true
    begin
      transaction do
        api_posts.each do |p|
          post = find_or_initialize_by(
            brightspace_id: p['PostId'].to_s,
            topic_id: topic_id.to_s
          )
          post.thread_id = p['ThreadId'].to_s
          post.parent_post_id = p['ParentPostId'].to_s if p['ParentPostId']
          post.subject = p['Subject']
          post.body = (p.dig('Body', 'Html') || p.dig('Body', 'Text')).to_s
          post.author_name = p['PostingUserDisplayName']
          post.author_id = p.dig('Author', 'Identifier') || p['UserId']
          post.posted_at = Time.zone.parse(p['DatePosted']) rescue nil
          post.is_instructor = p.dig('Author', 'IsInstructor') == true || p.dig('Author', 'RoleName').to_s =~ /Instructor/i rescue false
          
          post.save!
        end
      end
    ensure
      Thread.current[:skip_discussion_callbacks] = false
    end

    # One final update to the topic counts after all saves
    topic = DiscussionTopic.find_by(brightspace_id: topic_id.to_s)
    if topic
      topic.update_columns(
        thread_count: DiscussionPost.where(topic_id: topic_id.to_s).distinct.count(:thread_id),
        post_count: DiscussionPost.where(topic_id: topic_id.to_s).count,
        last_post_date: DiscussionPost.where(topic_id: topic_id.to_s).maximum(:posted_at)
      )
    end
  end
end
