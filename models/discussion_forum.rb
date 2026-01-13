class DiscussionForum < ActiveRecord::Base
  validates :brightspace_id, presence: true, uniqueness: { scope: :course_id }
  belongs_to :course, foreign_key: :course_id, primary_key: :org_unit_id
  has_many :discussion_topics, foreign_key: :forum_id, primary_key: :brightspace_id
end
