theory Keystone
  imports Main
begin

text \<open>
  A keystone fragment of the roommate security-deposit dispute.

  The thesis of the whole system in one file: the host logic holds the
  *formalizable* core backstage; the genuinely human question is *isolated*,
  never decided by the prover. Two jobs, both honest:

    (1) the ledger  — a calculator that cannot be lied to (~30%);
    (2) the crux    — the obligation collapses onto one contested predicate,
                       which the kernel hands back to the humans (~70%).

  No deontic modality yet — that arrives by swapping the toy @{text lease_term}
  for the CondNormReasHOL / LogiKEy dyadic-deontic embedding. This stone only
  has to prove the *shape* is real and checks.
\<close>

section \<open>(1) The ledger: the part that cannot be lied to\<close>

definition deposit        :: int where "deposit        = 1200"
definition cleaning       :: int where "cleaning       = 150"
definition carpet_repair  :: int where "carpet_repair  = 300"

definition itemized_total :: int where "itemized_total = cleaning + carpet_repair"
definition refund_due     :: int where "refund_due     = deposit - itemized_total"

text \<open>From the stipulated facts there is exactly one consistent balance.\<close>
lemma ledger_balances: "refund_due = 750"
  by (simp add: refund_due_def itemized_total_def deposit_def cleaning_def carpet_repair_def)

text \<open>
  If a party *claims* a total the itemization does not support, the host refutes
  it. The LLM cannot hallucinate a number past the prover — and neither can a
  motivated roommate.
\<close>
lemma claimed_total_is_false:
  assumes "(claimed_total :: int) = 500"
  shows   "claimed_total \<noteq> itemized_total"
  using assms by (simp add: itemized_total_def cleaning_def carpet_repair_def)

section \<open>(2) The crux: everything reduces to one human question\<close>

text \<open>
  The lease makes the tenant bear the cost of *damage* but not of ordinary
  wear. Whether the carpet stain is damage or wear is the genuinely contested,
  un-formalizable predicate. The kernel does not decide it. It proves the entire
  obligation is equivalent to it — handing the parties exactly the one thing
  they must actually resolve between themselves. Subtraction, made formal.
\<close>
locale lease =
  fixes stain_is_damage   :: bool
    and tenant_owes_carpet :: bool
  assumes lease_term: "tenant_owes_carpet \<longleftrightarrow> stain_is_damage"
begin

lemma crux: "tenant_owes_carpet \<longleftrightarrow> stain_is_damage"
  by (rule lease_term)

text \<open>Two coherent worlds, exactly one contested bit between them: the dispute, located.\<close>
lemma damage_world: "stain_is_damage   \<Longrightarrow>   tenant_owes_carpet" using lease_term by simp
lemma wear_world:   "\<not> stain_is_damage \<Longrightarrow> \<not> tenant_owes_carpet" using lease_term by simp

end

end
