class AddTargetGradeToCourses < ActiveRecord::Migration[5.2]
  def change
    add_column :courses, :target_grade, :float, default: 93.0
  end
end
