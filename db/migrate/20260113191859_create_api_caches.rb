class CreateApiCaches < ActiveRecord::Migration[5.2]
  def change
    create_table :api_caches do |t|
      t.string :path, index: true, unique: true
      t.text :data
      t.timestamps
    end
  end
end
