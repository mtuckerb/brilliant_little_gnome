class Grade < ActiveRecord::Base
  validates :brightspace_id, presence: true, uniqueness: { scope: :course_id }
  belongs_to :course, foreign_key: :course_id, primary_key: :org_unit_id

  def percentage
    return 0 if denominator.nil? || denominator == 0
    ((numerator.to_f / denominator.to_f) * 100).round(2)
  end

  def is_graded?
    # Brightspace often returns 0/X for ungraded items or items not yet submitted.
    # We check if numerator is not nil and displayed_grade is not empty or a placeholder
    !numerator.nil? && !(displayed_grade.nil? || displayed_grade.strip == "" || displayed_grade == "-")
  end

  def self.calculate_weighted_total(course_id)
    grades = where(course_id: course_id.to_s)
    
    # User Request: "Points determine weight (it is directly proportional)."
    # In a points-based system, the denominator IS the weight.
    
    graded_items = grades.select { |g| g.is_graded? && (g.denominator || 0) > 0 }
    
    if graded_items.empty?
      all_possible_points = (grades.where(is_extra_credit: false).sum(:denominator) || 0).to_f
      return { 
        score: 0, 
        confidence: 0, 
        total_points_earned: 0, 
        total_points_possible: 0,
        all_possible_points: all_possible_points,
        remaining_points: all_possible_points,
        max_potential_score: 100.0, # If nothing graded, potential is still 100
        required_avg: nil
      } 
    end

    # Total points earned include extra credit numerators
    total_points_earned = graded_items.sum { |g| g.numerator || 0 }
    
    # Total points possible EXCLUDE extra credit denominators
    total_points_possible = graded_items.reject(&:is_extra_credit).sum { |g| g.denominator || 0 }
    
    # All points in the syllabus (graded or not) EXCLUDE extra credit
    all_possible_points = (grades.where(is_extra_credit: false).sum(:denominator) || 0).to_f
    
    # Current score can exceed 100% if extra credit is high
    current_score = total_points_possible > 0 ? (total_points_earned.to_f / total_points_possible.to_f) * 100 : 100.0
    
    # Baseline Confidence: what percent of the total points have been graded
    base_confidence = all_possible_points > 0 ? (total_points_possible.to_f / all_possible_points) * 100 : 0
    
    # Updated Confidence logic:
    # If total points in syllabus >= 100, we trust the points-based percentage directly.
    # Otherwise, we apply the item-count penalty to ensure low confidence for sparse data.
    if all_possible_points >= 100
      confidence = base_confidence
    else
      item_count_multiplier = [graded_items.count / 3.0, 1.0].min
      confidence = base_confidence * item_count_multiplier
    end

    remaining_points = all_possible_points - total_points_possible
    max_potential_score = all_possible_points > 0 ? ((total_points_earned + remaining_points) / all_possible_points) * 100 : 100.0

    # Projection Math
    target_grade = Course.find_by(org_unit_id: course_id)&.target_grade || 93.0
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

  def self.to_gpa(score)
    return 0.0 if score.nil?
    # Based on University of Maine System / USM GPA Scale
    if score >= 93
      4.00
    elsif score >= 90
      3.67
    elsif score >= 87
      3.33
    elsif score >= 83
      3.00
    elsif score >= 80
      2.67
    elsif score >= 77
      2.33
    elsif score >= 73
      2.00
    elsif score >= 70
      1.67
    elsif score >= 67
      1.33
    elsif score >= 63
      1.00
    elsif score >= 60
      0.67
    else
      0.00
    end
  end
end
