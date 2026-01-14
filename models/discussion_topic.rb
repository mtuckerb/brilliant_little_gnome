class DiscussionTopic < ActiveRecord::Base
  validates :brightspace_id, presence: true, uniqueness: { scope: :forum_id }
  belongs_to :discussion_forum, foreign_key: :forum_id, primary_key: :brightspace_id
  has_many :discussion_posts, foreign_key: :topic_id, primary_key: :brightspace_id
  has_many :discussion_threads, foreign_key: :topic_id, primary_key: :brightspace_id

  def display_thread_count
    (thread_count && thread_count > 0) ? thread_count : discussion_posts.select(:thread_id).distinct.count
  end

  def display_post_count
    (post_count && post_count > 0) ? post_count : discussion_posts.count
  end
end
