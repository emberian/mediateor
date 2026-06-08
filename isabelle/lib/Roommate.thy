theory Roommate
  imports Mediator
begin

text \<open>
  \<^bold>\<open>Roommate\<close> — the carpet scenario instantiated on the normative layer.

  This mirrors @{theory_text Keystone} (the arithmetic keystone) but with REAL
  obligations: the carpet rule is a Poole-style defeasible default discharged
  through the Anderson/Kanger deontic operator from @{theory_text Mediator},
  not a bare biconditional.

  The data is @{file \<open>../../scenarios/roommate.json\<close>}: Sam claims the carpet
  stain is chargeable damage and that Robin therefore owes the repair; Robin
  claims it is ordinary wear and tear (the more-specific exception).

  We prove three things, in the system's own voice:
    (a) by default, given damage, the tenant is liable (and obliged);
    (b) under the wear-and-tear exception that obligation is DEFEATED;
    (c) the whole question still reduces to one contested predicate — the crux
        is @{term stain_is_damage}, handed back to the humans, never decided.
\<close>

text \<open>
  The lease, as a locale (mirroring @{theory_text Keystone.lease}). The lease
  term ties the tenant's carpet liability to the damage/wear distinction. Here
  liability is the OUTPUT of the defeasible default machinery, with the wear
  exception wired in as the more-specific defeater.
\<close>
locale carpet_lease =
  fixes stain_is_damage    :: bool   \<comment> \<open>Sam's claim s1 / Robin's claim r1 (the crux)\<close>
    and stain_is_wear      :: bool   \<comment> \<open>the more-specific exception condition\<close>
  \<comment> \<open>The lease's own stipulation: a stain is wear precisely when it is not
      chargeable damage. This is the shared ground both parties stipulate
      (the @{text stipulated} biconditional, recast for the default layer).\<close>
  assumes wear_iff: "stain_is_wear \<longleftrightarrow> \<not> stain_is_damage"
begin

text \<open>The tenant's liability under the carpet default, with the wear exception
  acting as the Poole-specific blocker.\<close>
definition tenant_owes_carpet :: bool where
  "tenant_owes_carpet \<equiv> liable_under stain_is_damage (wear_blocks stain_is_wear)"

text \<open>
  \<^bold>\<open>(a) Default liability + obligation.\<close> In the damage world (Sam's reading:
  damage holds, hence by @{thm wear_iff} no wear, hence nothing blocks), the
  carpet default fires and the tenant is liable — and that liability is a
  genuine Anderson/Kanger obligation.
\<close>
lemma damage_world_liable:
  assumes "stain_is_damage"
  shows   "tenant_owes_carpet"
  unfolding tenant_owes_carpet_def liable_under_def
            applies_default_def wear_blocks_def
  using assms wear_iff by simp

lemma damage_world_obliged:
  assumes "stain_is_damage"
  shows   "O\<langle>tenant_owes_carpet\<rangle>"
  using damage_world_liable[OF assms] surviving_default_is_obligation
  by (simp add: tenant_owes_carpet_def carpet_obligation_def)

text \<open>
  \<^bold>\<open>(b) The exception defeats the obligation.\<close> In the wear world (Robin's
  reading: ordinary wear, hence no chargeable damage), the more-specific
  exception blocks the default. The liability conclusion is withdrawn and the
  positive deontic force evaporates — its obligation reduces to the bare
  sanction constant, i.e. no force from the (now-defeated) default.
\<close>
lemma wear_world_not_liable:
  assumes "stain_is_wear"
  shows   "\<not> tenant_owes_carpet"
  unfolding tenant_owes_carpet_def liable_under_def
            applies_default_def wear_blocks_def
  using assms by simp

lemma wear_world_obligation_defeated:
  assumes "stain_is_wear"
  shows   "O\<langle>tenant_owes_carpet\<rangle> \<longleftrightarrow> Viol"
  using wear_world_not_liable[OF assms]
  by (simp add: Obl_def)

text \<open>
  \<^bold>\<open>Non-monotonicity across the two worlds.\<close> The SAME lease yields liability in
  the damage world and withdraws it in the wear world — the default was
  genuinely defeated, not merely absent.
\<close>
lemma carpet_default_is_defeasible:
  "(stain_is_damage \<longrightarrow> tenant_owes_carpet)
     \<and> (stain_is_wear \<longrightarrow> \<not> tenant_owes_carpet)"
  using damage_world_liable wear_world_not_liable by blast

text \<open>
  \<^bold>\<open>(c) The crux.\<close> Everything reduces to one contested predicate. The tenant's
  carpet liability holds precisely when the stain is chargeable damage — the
  obligation collapses onto @{term stain_is_damage}, exactly the bit the kernel
  hands back to the humans and refuses to decide. This is @{theory_text Keystone}'s
  @{text crux} re-derived through real obligations rather than a stipulated
  biconditional.
\<close>
lemma crux: "tenant_owes_carpet \<longleftrightarrow> stain_is_damage"
  unfolding tenant_owes_carpet_def liable_under_def
            applies_default_def wear_blocks_def
  using wear_iff by blast

text \<open>
  And the deontic form of the crux: the obligation to pay has positive force
  exactly in the damage world. The kernel proves the obligation is equivalent
  to the crux predicate (modulo the sanction constant) and then stops.
\<close>
lemma crux_deontic:
  assumes "\<not> Viol"
  shows   "O\<langle>tenant_owes_carpet\<rangle> \<longleftrightarrow> stain_is_damage"
  using assms crux by (auto simp: Obl_def)

text \<open>
  Two coherent worlds, exactly one contested bit between them (mirroring
  @{theory_text Keystone}'s @{text damage_world} / @{text wear_world}).
\<close>
lemma damage_world: "stain_is_damage   \<Longrightarrow>   tenant_owes_carpet"
  by (rule damage_world_liable)
lemma wear_world:   "stain_is_wear     \<Longrightarrow> \<not> tenant_owes_carpet"
  by (rule wear_world_not_liable)

end

text \<open>
  The lease is satisfiable in both worlds — the embedding does not secretly
  pin the crux. Instantiating with @{term "stain_is_damage = True"} and with
  @{term "stain_is_damage = False"} both discharge the locale assumption.
\<close>
interpretation damage_case: carpet_lease True False
  by unfold_locales simp

interpretation wear_case: carpet_lease False True
  by unfold_locales simp

text \<open>Sanity: the two instantiations land on opposite liabilities, as they must.\<close>
lemma worlds_disagree:
  "damage_case.tenant_owes_carpet \<and> \<not> wear_case.tenant_owes_carpet"
  by (simp add: damage_case.tenant_owes_carpet_def
                wear_case.tenant_owes_carpet_def
                liable_under_def applies_default_def wear_blocks_def)

end
