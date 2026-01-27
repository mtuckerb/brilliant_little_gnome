class AddUniqueIndicesAndOverviewRaw < ActiveRecord::Migration[7.2]
  def change
    # 1. Add overview_raw to courses if it doesn't exist
    unless column_exists?(:courses, :overview_raw)
      add_column :courses, :overview_raw, :text
    end

    # 2. Add unique indices for upsert_all support
    
    # Notifications
    unless index_exists?(:notifications, [:course_id, :external_id], unique: true)
      add_index :notifications, [:course_id, :external_id], unique: true, name: 'index_notifications_on_course_and_external_id'
    end

    # Assignments
    unless index_exists?(:assignments, [:course_id, :brightspace_id], unique: true)
      add_index :assignments, [:course_id, :brightspace_id], unique: true, name: 'index_assignments_on_course_and_bs_id'
    end

    # Content Modules
    unless index_exists?(:content_modules, [:course_id, :brightspace_id], unique: true)
      add_index :content_modules, [:course_id, :brightspace_id], unique: true, name: 'index_content_modules_on_course_and_bs_id'
    end

    # Content Items
    unless index_exists?(:content_items, [:module_id, :brightspace_id], unique: true)
      add_index :content_items, [:module_id, :brightspace_id], unique: true, name: 'index_content_items_on_module_and_bs_id'
    end

    # Grades
    unless index_exists?(:grades, [:course_id, :brightspace_id], unique: true)
      add_index :grades, [:course_id, :brightspace_id], unique: true, name: 'index_grades_on_course_id_and_brightspace_id_unique'
    end
  end
end
