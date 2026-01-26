class Course < ActiveRecord::Base
  include HasUserIdentity
  validates :org_unit_id, presence: true, uniqueness: true
  has_many :content_modules, foreign_key: :course_id, primary_key: :org_unit_id
  has_many :assignments, foreign_key: :course_id, primary_key: :org_unit_id

  def end_of_week_date(reference_date = Time.now)
    # end_of_week_day is 0 for Sunday, 1 for Monday, etc.
    # ActiveSupport's end_of_week defaults to Sunday (0)
    # But it doesn't allow passing a custom day easily in older versions
    # So we calculate it manually.
    
    # reference_date.wday: 0 (Sun) to 6 (Sat)
    # target_day: 0 (Sun) to 6 (Sat)
    target_day = self.end_of_week_day || 0
    days_to_add = (target_day - reference_date.wday) % 7
    (reference_date + days_to_add.days).change(hour: 23, min: 59, sec: 59)
  end
end
