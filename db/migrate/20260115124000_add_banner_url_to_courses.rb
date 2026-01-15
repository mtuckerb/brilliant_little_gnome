class AddBannerUrlToCourses < ActiveRecord::Migration[5.2]
  def change
    add_column :courses, :banner_url, :string
  end
end
