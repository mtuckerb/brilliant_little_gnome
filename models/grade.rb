class Grade < ActiveRecord::Base
  include HasUserIdentity
  validates :brightspace_id, presence: true, uniqueness: { scope: :course_id }
  belongs_to :course, foreign_key: :course_id, primary_key: :org_unit_id

  def percentage
    num = effective_numerator
    den = effective_denominator
    return 0 if den.nil? || den == 0
    ((num.to_f / den.to_f) * 100).round(2)
  end

  def is_graded?
    return false if manually_marked_ungraded
    return true unless numerator.nil?
    return false if displayed_grade.nil?
    displayed_grade.include?('%') || displayed_grade.include?('/')
  end

  def effective_numerator
    return numerator if numerator
    return nil if displayed_grade.nil?
    
    # Try parsing "90 %"
    match = displayed_grade.match(/(\d+(\.\d+)?)\s*%/)
    return match[1].to_f if match
    
    # Try parsing "18 / 20"
    match = displayed_grade.match(/(\d+(\.\d+)?)\s*\/\s*(\d+(\.\d+)?)/)
    return match[1].to_f if match
    
    nil
  end

  def effective_denominator
    return denominator if denominator
    return nil if displayed_grade.nil?
    
    # If it's a percentage, assume 100
    return 100.0 if displayed_grade.include?('%')
    
    # Try parsing "18 / 20" - capture group 3 is the denominator
    match = displayed_grade.match(/(\d+(\.\d+)?)\s*\/\s*(\d+(\.\d+)?)/)
    return match[3].to_f if match
    
    nil
  end

  def self.calculate_weighted_total(course_id)
    _calculate_weighted_total(course_id)
  end

  def self._calculate_weighted_total(course_id)
    grades = where(course_id: course_id.to_s)
    course = Course.find_by(org_unit_id: course_id)
    completed_course = completed_past_semester_course?(course)
    graded_items = grades.select { |g| g.is_graded? && (g.effective_denominator || 0) > 0 }
    
    if graded_items.empty?
      non_extra_credit_grades = grades.reject(&:is_extra_credit)
      all_possible_points = non_extra_credit_grades.sum { |g| g.weight || g.effective_denominator || 0 }.to_f
      return { 
        score: nil, 
        confidence: 0, 
        total_points_earned: 0, 
        total_points_possible: 0,
        all_possible_points: all_possible_points,
        remaining_points: all_possible_points,
        max_potential_score: 100.0,
        required_avg: nil
      } 
    end

    # Determine if we should use weights or points
    # Only use weighted calculation if NON-extra-credit items have meaningful weights
    # If only extra-credit items have weights, fall back to points-based
    non_extra_credit_graded = graded_items.reject(&:is_extra_credit)
    use_weights = non_extra_credit_graded.any? { |g| (g.weight || 0) > 0 }

    # Items that have actual data (including manually ungraded)
    data_items = grades.select { |g| (g.numerator || g.effective_numerator) && (g.effective_denominator || 0) > 0 }

    if use_weights
      # Weighted calculation
      total_weight_earned = graded_items.sum { |g| (g.percentage / 100.0) * (g.weight || 0) }
      total_weight_possible = graded_items.reject(&:is_extra_credit).sum { |g| g.weight || 0 }
      
      all_possible_weight = grades.reject(&:is_extra_credit).sum { |g| g.weight || 0 }
      all_possible_weight = 100.0 if all_possible_weight < 100.0 # Standardize to 100% if incomplete
      
      current_score = total_weight_possible > 0 ? (total_weight_earned / total_weight_possible) * 100 : 0
      
      # Recalculated confidence: points_scored (earned weight) / total_points (all possible weight)
      weight_scored = data_items.sum { |g| (g.percentage / 100.0) * (g.weight || 0) }
      confidence = (weight_scored / all_possible_weight) * 100

      remaining_points = all_possible_weight - total_weight_possible
      max_potential_score = ((total_weight_earned + remaining_points) / all_possible_weight) * 100
      # ...

      confidence = 100.0 if completed_course

      target_grade = course&.target_grade || 93.0
      points_needed = [ (target_grade / 100.0 * all_possible_weight) - total_weight_earned, 0 ].max
      required_avg = remaining_points > 0 ? (points_needed / remaining_points) * 100 : nil
      
      return {
        score: current_score.round(2),
        confidence: confidence.round(1),
        total_points_earned: total_weight_earned.round(2),
        total_points_possible: total_weight_possible.round(2),
        all_possible_points: all_possible_weight.round(2),
        remaining_points: remaining_points.round(2),
        max_potential_score: max_potential_score.round(2),
        target_grade: target_grade,
        required_avg: required_avg ? required_avg.round(2) : nil,
        is_impossible: required_avg && required_avg > 100,
        points_needed: points_needed.round(2),
        item_count: graded_items.count
      }
    end

    total_points_earned = graded_items.sum { |g| g.effective_numerator || 0 }
    total_points_possible = graded_items.reject(&:is_extra_credit).sum { |g| g.effective_denominator || 0 }
    
    # Calculate all possible points using effective denominators
    non_extra_credit_grades = grades.reject(&:is_extra_credit)
    all_possible_points = non_extra_credit_grades.sum { |g| g.effective_denominator || 0 }.to_f
    
    # If we have only percentage items (no denominators), all_possible_points might be 0
    # but we should still calculate an average score.
    if total_points_possible == 0 && !graded_items.empty?
      # Fallback to simple average of percentages if we can't weight by points
      scores = graded_items.map { |g| g.effective_numerator.to_f }
      current_score = scores.sum / scores.size.to_f
    else
      current_score = total_points_possible > 0 ? (total_points_earned.to_f / total_points_possible.to_f) * 100 : 100.0
    end

    # Recalculated confidence: points_scored (all earned) / total_points (all possible)
    all_points_scored = data_items.sum { |g| g.effective_numerator || 0 }
    confidence = all_possible_points > 0 ? (all_points_scored.to_f / all_possible_points) * 100 : 0
    
    remaining_points = all_possible_points - total_points_possible
    max_potential_score = all_possible_points > 0 ? ((total_points_earned + remaining_points) / all_possible_points) * 100 : 100.0

    confidence = 100.0 if completed_course

    target_grade = course&.target_grade || 93.0
    required_points_to_hit_target = (target_grade / 100.0) * all_possible_points
    points_needed = [required_points_to_hit_target - total_points_earned, 0].max
    
    required_avg = remaining_points > 0 ? (points_needed / remaining_points) * 100 : nil
    is_impossible = required_avg && required_avg > 100

    {
      score: current_score.round(2),
      confidence: confidence.round(1),
      total_points_earned: total_points_earned.round(2),
      total_points_possible: total_points_possible.round(2),
      all_possible_points: all_possible_points.round(2),
      remaining_points: remaining_points.round(2),
      max_potential_score: max_potential_score.round(2),
      target_grade: target_grade,
      required_avg: required_avg ? required_avg.round(2) : nil,
      is_impossible: is_impossible,
      points_needed: points_needed.round(2),
      item_count: graded_items.count
    }
  end

  def self.completed_past_semester_course?(course)
    return false unless course&.semester
    return false if course.dropped?

    latest_semester = Course.where.not(semester: [nil, '']).pluck(:semester).max_by { |semester| semester_weight(semester) }
    return false unless latest_semester

    semester_weight(course.semester) < semester_weight(latest_semester)
  end

  def self.semester_weight(semester)
    return 0 unless semester

    year_match = semester.match(/\d{4}/)
    return 0 unless year_match

    season_weight = case semester
                    when /Winter/i then 1
                    when /Spring/i then 2
                    when /Summer/i then 3
                    when /Fall/i then 4
                    else 0
                    end

    (year_match[0].to_i * 10) + season_weight
  end

  def self.to_gpa(score)
    return 0.0 if score.nil?
    if score >= 93 then 4.00
    elsif score >= 90 then 3.67
    elsif score >= 87 then 3.33
    elsif score >= 83 then 3.00
    elsif score >= 80 then 2.67
    elsif score >= 77 then 2.33
    elsif score >= 73 then 2.00
    elsif score >= 70 then 1.67
    elsif score >= 67 then 1.33
    elsif score >= 63 then 1.00
    elsif score >= 60 then 0.67
    else 0.00
    end
  end
end
