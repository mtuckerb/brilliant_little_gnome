class DiscussionTopic < ActiveRecord::Base
  validates :brightspace_id, presence: true, uniqueness: { scope: :forum_id }
  belongs_to :course, foreign_key: :course_id, primary_key: :org_unit_id
  belongs_to :discussion_forum, foreign_key: :forum_id, primary_key: :brightspace_id
  has_many :discussion_threads, foreign_key: :topic_id, primary_key: :brightspace_id
end
