class AddManuallyMarkedUngradedToGrades < ActiveRecord::Migration[7.2]
  def change
    add_column :grades, :manually_marked_ungraded, :boolean, default: false
  end
end
