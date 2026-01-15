class CreateNormalizedTables < ActiveRecord::Migration[5.2]
  def change
    create_table :courses do |t|
      t.string :org_unit_id, index: true, unique: true
      t.string :name
      t.string :code
      t.string :semester
      t.datetime :last_accessed_at
      t.boolean :is_pinned, default: false
      t.timestamps
    end

    create_table :content_modules do |t|
      t.string :course_id, index: true
      t.string :brightspace_id, index: true
      t.string :title
      t.text :description
      t.integer :sort_order
      t.string :parent_id, index: true
      t.timestamps
    end

    create_table :content_items do |t|
      t.string :module_id, index: true
      t.string :brightspace_id, index: true
      t.string :title
      t.string :item_type
      t.string :url
      t.boolean :is_hidden, default: false
      t.timestamps
    end

    create_table :assignments do |t|
      t.string :course_id, index: true
      t.string :brightspace_id, index: true
      t.string :name
      t.datetime :due_date
      t.text :description
      t.boolean :is_graded, default: false
      t.timestamps
    end
  end
end
