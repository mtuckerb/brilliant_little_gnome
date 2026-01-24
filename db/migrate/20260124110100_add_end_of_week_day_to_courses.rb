class AddEndOfWeekDayToCourses < ActiveRecord::Migration[7.2]
  def change
    add_column :courses, :end_of_week_day, :integer, default: 0
  end
end
