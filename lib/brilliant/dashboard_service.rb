module Brilliant
  class DashboardService
    def self.get_summary_data(user_prefs)
      courses = Course.all.order(is_pinned: :desc, last_accessed_at: :desc)
      all_semesters = courses.map(&:semester).compact.uniq.sort_by { |s| semester_weight(s) }
      latest_semester = all_semesters.last
      overview_semester = user_prefs.default_semester || latest_semester
      
      semester_grades = []
      total_weighted_points = 0.0
      total_units_count = 0
      cumulative_points_earned = 0.0
      cumulative_points_possible = 0.0
      
      if overview_semester
        overview_weight = semester_weight(overview_semester)
        courses.each do |c|
          next unless c.semester
          c_weight = semester_weight(c.semester)
          next if c_weight > overview_weight
          
          stats = Grade.calculate_weighted_total(c.org_unit_id) rescue nil
          next unless stats
          
          if c.semester == overview_semester
            semester_grades << { course: c, stats: stats }
          end

          target = c.target_grade || 93.0
          score_to_use = (c_weight == overview_weight) ? 
            (stats[:all_possible_points] > 0 ? ((stats[:total_points_earned] + (stats[:all_possible_points] - stats[:total_points_possible]) * (target/100.0)) / stats[:all_possible_points] * 100.0) : target) : 
            stats[:score]

          if stats[:total_points_possible] > 0 || c_weight == overview_weight
            cumulative_points_earned += stats[:total_points_earned]
            cumulative_points_possible += stats[:total_points_possible]
            
            course_units = c.units || 3
            total_weighted_points += (Grade.to_gpa(score_to_use) * course_units)
            total_units_count += course_units
          end
        end
      end

      historic_gpa = user_prefs.historic_gpa || 0.0
      historic_units = user_prefs.historic_units || 0
      overall_gpa = (total_units_count + historic_units) > 0 ? 
        ((total_weighted_points + (historic_gpa * historic_units)) / (total_units_count + historic_units)) : 0.0

      total_max_weighted_points = (historic_gpa * historic_units) + total_weighted_points - (semester_grades.sum { |sg| Grade.to_gpa(sg[:stats][:score]) * (sg[:course].units || 3) })
      semester_grades.each do |sg|
        course_units = sg[:course].units || 3
        max_course_gpa = Grade.to_gpa(sg[:stats][:max_potential_score])
        total_max_weighted_points += (max_course_gpa * course_units)
      end
      max_potential_gpa = (total_units_count + historic_units) > 0 ? (total_max_weighted_points / (total_units_count + historic_units)) : 0.0

      {
        overall_gpa: overall_gpa,
        max_potential_gpa: max_potential_gpa,
        semester: overview_semester,
        semester_grades: semester_grades,
        all_semesters: all_semesters,
        cumulative_points_earned: cumulative_points_earned,
        cumulative_points_possible: cumulative_points_possible
      }
    end

    private

    def self.semester_weight(sem)
      return 0 unless sem
      year_match = sem.match(/\d{4}/)
      return 0 unless year_match
      year = year_match[0].to_i
      
      season_weight = 0
      if sem =~ /Winter/i
        season_weight = 1
      elsif sem =~ /Spring/i
        season_weight = 2
      elsif sem =~ /Summer/i
        season_weight = 3
      elsif sem =~ /Fall/i
        season_weight = 4
      end
      
      (year * 10) + season_weight
    end
  end
end
