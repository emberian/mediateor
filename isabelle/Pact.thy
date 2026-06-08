theory Pact
  imports Main
begin

text \<open>
  \<^bold>\<open>Forward constitution\<close> (certified pact) — the load-bearing experiment.

  Can we, at \<^emph>\<open>design time\<close> — before any dispute — prove a two-party agreement
  is \<^bold>\<open>complete\<close> (every foreseeable world fires some clause) and \<^bold>\<open>non-contradictory\<close>
  (no world fires two clauses demanding conflicting outcomes), while the human
  value-predicate stays \<^emph>\<open>uninterpreted\<close> — a FREE const, handed back \<open>Unknown\<close> at
  dispute time? The goals are \<^bold>\<open>universally quantified over the free crux predicate\<close>;
  that universal quantifier is exactly what lets the certificate be issued before
  the fight, while the moral question stays the humans'.

  THE RISK TO RETIRE FIRST: do coverage / consistency stay inside the decidable
  fragment the real Isabelle gate closes (\<open>auto\<close>/\<open>presburger\<close>) when the clause guards
  mix a FREE boolean crux with integer (cents/days) arithmetic? This theory is that
  experiment, hand-written, on a roommate move-out pact.
\<close>

text \<open>
  Declared question-space of the pact:
   \<^item> \<open>stain_is_damage\<close> — the contested value predicate (the future crux): FREE.
   \<^item> \<open>notice_days\<close>     — an agreed, measurable integer (days' notice the tenant gave).
  The cliff (30 days) is a stipulated constant both sides accept up front.
\<close>

definition g_clean :: "bool \<Rightarrow> int \<Rightarrow> bool" where
  "g_clean stain_is_damage notice_days \<equiv> notice_days \<ge> 30 \<and> \<not> stain_is_damage"

definition g_damaged :: "bool \<Rightarrow> int \<Rightarrow> bool" where
  "g_damaged stain_is_damage notice_days \<equiv> notice_days \<ge> 30 \<and> stain_is_damage"

definition g_short :: "bool \<Rightarrow> int \<Rightarrow> bool" where
  "g_short stain_is_damage notice_days \<equiv> notice_days < 30"

text \<open>The refund (in cents) each clause awards. Distinct amounts, so a clash of
  clauses would be a clash of \<^emph>\<open>numbers\<close> — checkable.\<close>
definition o_clean   :: "int \<Rightarrow> bool" where "o_clean   refund \<equiv> refund = 120000"
definition o_damaged :: "int \<Rightarrow> bool" where "o_damaged refund \<equiv> refund =  90000"
definition o_short   :: "int \<Rightarrow> bool" where "o_short   refund \<equiv> refund = 100000"

section \<open>(A) Coverage — the pact handles every declared world\<close>

text \<open>The disjunction of the guards is VALID, universally over the free crux and
  the integer: no foreseeable (declared) world is left unhandled. THIS is the goal
  the gate must close with the value-predicate left free.\<close>
theorem pact_coverage:
  "\<forall>(stain_is_damage::bool) (notice_days::int).
       g_clean   stain_is_damage notice_days
     \<or> g_damaged stain_is_damage notice_days
     \<or> g_short   stain_is_damage notice_days"
  by (auto simp: g_clean_def g_damaged_def g_short_def)

section \<open>(B) Consistency — no world fires two clauses with conflicting refunds\<close>

text \<open>Stated in the general shape the codegen would emit:
  \<open>\<not> (g\<^sub>i \<and> g\<^sub>j \<and> o\<^sub>i refund \<and> \<not> o\<^sub>j refund)\<close> — there is no world and refund where two
  clauses both fire and demand different amounts. (Here the guards are pairwise
  exclusive, so the conflict is impossible; the gate must see that.)\<close>

theorem pact_consistent_clean_damaged:
  "\<not> (g_clean s nd \<and> g_damaged s nd \<and> o_clean r \<and> \<not> o_damaged r)"
  by (auto simp: g_clean_def g_damaged_def o_clean_def o_damaged_def)

theorem pact_consistent_clean_short:
  "\<not> (g_clean s nd \<and> g_short s nd \<and> o_clean r \<and> \<not> o_short r)"
  by (auto simp: g_clean_def g_short_def o_clean_def o_short_def)

theorem pact_consistent_damaged_short:
  "\<not> (g_damaged s nd \<and> g_short s nd \<and> o_damaged r \<and> \<not> o_short r)"
  by (auto simp: g_damaged_def g_short_def o_damaged_def o_short_def)

section \<open>(C) Falsification — drop a clause and the gap is findable\<close>

text \<open>
  The honest half: a BROKEN pact must not certify. Drop the short-notice clause
  and completeness fails — and the gate would surface the uncovered world as an
  open subgoal. We prove (build-safely) that the gap \<^emph>\<open>exists\<close>: there is a concrete
  world (give fewer than 30 days' notice) that no remaining clause handles. This
  is the "Failed to finish proof, here is the uncovered world" outcome, witnessed.
\<close>
theorem pact_gap_if_short_clause_dropped:
  "\<exists>(stain_is_damage::bool) (notice_days::int).
       \<not> (g_clean   stain_is_damage notice_days
        \<or> g_damaged stain_is_damage notice_days)"
  by (rule exI[of _ True], rule exI[of _ 0],
      auto simp: g_clean_def g_damaged_def)

text \<open>
  And the dual sanity: with all three clauses present, the witness world IS now
  covered (the short clause catches it) — so the gap above is genuinely the
  dropped clause's fault, not a malformed pact.
\<close>
theorem pact_gap_closed_by_short_clause:
  "g_clean   True 0 \<or> g_damaged True 0 \<or> g_short True 0"
  by (auto simp: g_clean_def g_damaged_def g_short_def)

end
