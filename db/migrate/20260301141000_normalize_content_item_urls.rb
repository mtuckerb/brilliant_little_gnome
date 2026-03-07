class NormalizeContentItemUrls < ActiveRecord::Migration[7.2]
  def up
    execute <<~SQL
      UPDATE content_items
      SET url = 'https://courses.maine.edu' || url
      WHERE url IS NOT NULL
        AND url != ''
        AND url LIKE '/%';
    SQL
  end

  def down
    execute <<~SQL
      UPDATE content_items
      SET url = SUBSTR(url, LENGTH('https://courses.maine.edu') + 1)
      WHERE url LIKE 'https://courses.maine.edu/%';
    SQL
  end
end
