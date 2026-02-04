class AddStatusToCourses < ActiveRecord::Migration[7.2]
  def change
    add_column :courses, :status, :string, default: 'active' unless column_exists?(:courses, :status)
    add_column :courses, :dropped_at, :datetime unless column_exists?(:courses, :dropped_at)
  end
end
