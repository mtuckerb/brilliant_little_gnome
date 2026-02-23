class CreateCourses < ActiveRecord::Migration[6.0]
  def change
    unless table_exists?(:courses)
      create_table :courses, primary_key: :org_unit_id do |t|
        t.string :org_unit_id, null: false
        t.string :name
        t.string :code
        t.string :status
        t.boolean :is_frozen, default: false
        t.string :target_grade
        t.string :banner_url
        t.integer :sort_order
        t.timestamps
      end
    end
  end
end
