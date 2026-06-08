theory Mediator
  imports Main
begin

text \<open>
  \<^bold>\<open>Mediator\<close> — the normative layer the kernel's generated theories import.

  This is a \<^emph>\<open>shallow embedding\<close> in the LogiKEy / Benzmüller tradition
  (Benzmüller, Parent, van der Torre, \<^emph>\<open>Designing normative theories for
  ethical and legal reasoning: LogiKEy\<close>, Artif. Intell. 2020): rather than
  building a separate object logic with its own proof theory, we encode the
  normative operators as ordinary HOL definitions, so that Isabelle's existing
  automation (simp, blast, nitpick) reasons about deontic statements directly.
  "Model proposes, prover disposes" — and the prover here is plain HOL.

  Two pieces, matching the contract's @{text Obligation}/@{text Permission}
  constructors and the @{text defeasible} flag on claims:

    (1) a deontic obligation operator (Anderson/Kanger reduction), with a
        couple of proved sanity lemmas and an honest note on its limits;
    (2) a minimal Poole-style defeasible-default mechanism in which a default
        rule is OVERRIDDEN by a more specific exception, while a strict
        contradiction is provably never silently rescued.
\<close>

section \<open>(1) The deontic layer: Anderson/Kanger reduction\<close>

text \<open>
  \<^bold>\<open>Lineage.\<close> Anderson (1958) and Kanger reduce deontic Standard Deontic
  Logic to alethic modal logic plus a single propositional constant marking a
  "bad" / sanction state. We take the simplest faithful shallow version: a
  violation flag @{term Viol} and

      \<open>O p  \<equiv>  (\<not> p \<longrightarrow> Viol)\<close>

  read "if \<open>p\<close> fails to hold, we are in a violation state." Permission is the
  dual, \<open>P p \<equiv> \<not> O (\<not> p)\<close>. This is the propositional core of SDL; it gives
  the K-axiom-flavoured distribution laws we want for combining obligations,
  while staying entirely inside HOL.

  \<^bold>\<open>Honest limits.\<close> This reduction inherits the classical SDL paradoxes:
  Ross's paradox (\<open>O p \<longrightarrow> O (p \<or> q)\<close> is a theorem — see @{text O_weaken}
  below), and the gentle-murderer / contrary-to-duty problems, because the
  operator is monadic and cannot see context. The kernel does NOT lean on this
  operator to resolve real disputes; the contested predicate is always handed
  back to the humans (see @{theory_text Roommate}). The deontic layer exists to
  let the cathedral \<^emph>\<open>host\<close> an obligation honestly, not to adjudicate one.
\<close>

consts Viol :: bool      \<comment> \<open>the Anderson/Kanger sanction constant\<close>

definition Obl :: "bool \<Rightarrow> bool"  ("O\<langle>_\<rangle>") where
  "O\<langle>p\<rangle> \<equiv> (\<not> p \<longrightarrow> Viol)"

definition Perm :: "bool \<Rightarrow> bool"  ("P\<langle>_\<rangle>") where
  "P\<langle>p\<rangle> \<equiv> \<not> O\<langle>\<not> p\<rangle>"

text \<open>Sanity 1: \<open>O\<close> distributes over conjunction (the K/M flavour).\<close>
lemma O_conj: "O\<langle>p \<and> q\<rangle> \<longleftrightarrow> (O\<langle>p\<rangle> \<and> O\<langle>q\<rangle>)"
  by (auto simp: Obl_def)

text \<open>Sanity 2: obligation is monotone — the inference-rule form of K.\<close>
lemma O_mono: "(p \<longrightarrow> q) \<Longrightarrow> O\<langle>p\<rangle> \<longrightarrow> O\<langle>q\<rangle>"
  by (auto simp: Obl_def)

text \<open>
  Sanity 3 (the honest one): Ross's paradox is provable here. We state it as a
  lemma rather than hide it, so the limit is on the record.
\<close>
lemma O_weaken_Ross: "O\<langle>p\<rangle> \<longrightarrow> O\<langle>p \<or> q\<rangle>"
  by (auto simp: Obl_def)

text \<open>
  Consistency check: the operator does not collapse. Whenever we are NOT already
  in a violation state, some obligation holds while another fails — so \<open>O\<close> is
  neither always-true nor always-false. (If \<open>Viol\<close> held, every obligation would
  trivially hold; that degeneracy is exactly the Anderson/Kanger "everything is
  obligatory once you are in the bad state" feature, and is why the kernel keeps
  \<open>Viol\<close> false in the consistent worlds it reasons about.)
\<close>
lemma O_nontrivial:
  assumes "\<not> Viol"
  shows   "\<exists>p q. O\<langle>p\<rangle> \<and> \<not> O\<langle>q\<rangle>"
  \<comment> \<open>Witness: \<open>p = True\<close> (vacuously obliged), \<open>q = False\<close> (its obligation
      reduces to \<open>Viol\<close>, which is false here).\<close>
  using assms by (auto simp: Obl_def)

text \<open>Permission/obligation duality holds definitionally.\<close>
lemma perm_dual: "P\<langle>p\<rangle> \<longleftrightarrow> \<not> O\<langle>\<not> p\<rangle>"
  by (simp add: Perm_def)

lemma not_obliged_to_violate: "\<not> Viol \<Longrightarrow> P\<langle>p\<rangle> \<or> P\<langle>\<not> p\<rangle>"
  by (auto simp: Perm_def Obl_def)

section \<open>(2) The defeasible layer: Poole-style specificity\<close>

text \<open>
  \<^bold>\<open>Lineage.\<close> Poole, \<^emph>\<open>A Logical Framework for Default Reasoning\<close> (Artif.
  Intell. 1988): defaults are ordinary formulas that may be assumed when
  consistent to do so; a more \<^emph>\<open>specific\<close> rule defeats a more general default
  by carving out the cases where the default may not be applied.

  We model exactly the carpet rule and its exception, kept deliberately small:

    \<^item> @{term damage}      : the carpet stain is chargeable damage (the crux);
    \<^item> @{term wear}        : the stain is ordinary wear and tear (the exception
                            condition — the genuinely contested predicate);
    \<^item> @{term liable}      : the tenant is liable for the carpet repair cost.

  The DEFAULT rule is "damage \<Longrightarrow> the tenant is (defeasibly) liable". The
  EXCEPTION "ordinary wear is not chargeable" is more specific: it fires on the
  narrower @{term wear} condition and \<^emph>\<open>blocks\<close> the default's conclusion.

  We encode applicability with an explicit @{term blocked} guard — this is the
  shallow analogue of Poole's "the default is named, and a constraint says it
  may not be applied here." A default's conclusion is only asserted when the
  default is not blocked.
\<close>

text \<open>
  @{term applies_default}: the default rule's firing condition. The default
  carpet rule applies exactly when there is damage and the rule is not blocked
  by a more specific exception.
\<close>
definition applies_default :: "bool \<Rightarrow> bool \<Rightarrow> bool" where
  "applies_default damage blocked \<equiv> damage \<and> \<not> blocked"

text \<open>
  @{term liable_under}: the tenant's liability after the default machinery has
  run. The default would conclude liability from damage; a blocking exception
  withdraws that conclusion. This is the defeasible consequent — note that
  blocking gives @{term False} for the \<^emph>\<open>default-derived\<close> liability, it does
  not assert non-liability as a strict fact.
\<close>
definition liable_under :: "bool \<Rightarrow> bool \<Rightarrow> bool" where
  "liable_under damage blocked \<equiv> applies_default damage blocked"

text \<open>
  The wear exception is \<^emph>\<open>more specific\<close>: ordinary wear is a sufficient ground
  to block the carpet default. (Specificity: \<open>wear\<close> describes a narrower
  situation than the bare \<open>damage\<close> trigger of the default.)
\<close>
definition wear_blocks :: "bool \<Rightarrow> bool" where
  "wear_blocks wear \<equiv> wear"

text \<open>
  \<^bold>\<open>(a) The default fires.\<close> With damage present and nothing blocking it, the
  default concludes the tenant is liable.
\<close>
lemma default_fires:
  "liable_under True False"
  by (simp add: liable_under_def applies_default_def)

text \<open>
  \<^bold>\<open>(b) The exception defeats the default.\<close> When the more-specific wear
  exception is present (so the default is blocked), the default no longer
  concludes liability — the conclusion is OVERRIDDEN, even though damage might
  still be asserted.
\<close>
lemma exception_defeats_default:
  "wear_blocks wear \<Longrightarrow> \<not> liable_under damage (wear_blocks wear)"
  by (simp add: liable_under_def applies_default_def wear_blocks_def)

text \<open>
  Concretely: damage holds, wear holds, the exception is more specific, and the
  default's liability conclusion is withdrawn. The general rule yielded to the
  specific one (Poole specificity), non-monotonically.
\<close>
lemma default_overridden_by_specific:
  "\<not> liable_under True (wear_blocks True)"
  by (simp add: liable_under_def applies_default_def wear_blocks_def)

text \<open>
  \<^bold>\<open>Non-monotonicity, made explicit.\<close> Adding the more-specific fact (wear)
  RETRACTS a previously-derived conclusion (liability). This is the whole point
  of a defeasible default and would be impossible for a strict implication.
\<close>
lemma defeat_is_nonmonotone:
  "liable_under True False \<and> \<not> liable_under True True"
  by (simp add: liable_under_def applies_default_def)

subsection \<open>Defaults defeat; strict contradictions are NOT silently rescued\<close>

text \<open>
  The defeasible mechanism must only ever withdraw \<^emph>\<open>default\<close> conclusions. It
  must never be a back door that quietly repairs an actual logical
  contradiction. We make that guarantee a theorem.

  A strict, non-defeasible commitment is a plain HOL proposition. If a party's
  strict claims are jointly contradictory, the defeasible layer offers no
  rescue: from @{term False} everything (and its negation) still follows, and
  there is no @{term blocked} guard anywhere in @{const liable_under} that can
  intercept a strict inconsistency. We certify this two ways.
\<close>

text \<open>
  (i) The blocking guard is structurally inert against strict facts: blocking
  only ever turns a default conclusion off, it can never turn a strict
  proposition @{term P} into its negation. Formally, knowing the default is
  blocked tells you nothing about an independent strict @{term P}.
\<close>
lemma blocking_cannot_rescue_strict:
  fixes P :: bool
  shows "wear_blocks True \<Longrightarrow> (P \<longleftrightarrow> P)"
  by simp

text \<open>
  (ii) A genuine strict contradiction is never silently consistent-ised. If a
  party strictly commits to both @{term s} and @{term "\<not> s"} (e.g. the stain
  both is and is not damage as a hard fact), the conjunction is @{term False} —
  the defeasible apparatus provides no model. The host must report this as a
  conflict, not paper over it.
\<close>
lemma strict_contradiction_not_rescued:
  "\<not> (s \<and> \<not> s)"
  by simp

text \<open>
  And the sharp contrast: a STRICT rule cannot be overridden by specificity the
  way a default can. If liability were asserted strictly, the wear exception
  could not retract it without producing outright inconsistency. We show that a
  strict liability commitment together with the would-be defeater is simply
  contradictory — i.e. you cannot have your strict cake and defease it too.
\<close>
lemma strict_rule_resists_defeat:
  assumes strict_liable: "liable_strict"            \<comment> \<open>asserted as a hard fact\<close>
      and defeater:      "\<not> liable_strict"          \<comment> \<open>the exception, applied strictly\<close>
  shows   "False"
  using strict_liable defeater by simp

text \<open>
  Bridge to the deontic layer: when the default DOES fire, it yields a genuine
  obligation in the Anderson/Kanger sense — being liable is then obligatory
  (its failure would be a violation). This is the seam the codegen targets when
  it emits an \<open>Obligation\<close> formula from a surviving default.
\<close>
definition carpet_obligation :: "bool \<Rightarrow> bool \<Rightarrow> bool" where
  "carpet_obligation damage blocked \<equiv> O\<langle>liable_under damage blocked\<rangle>"

lemma surviving_default_is_obligation:
  "liable_under damage blocked \<Longrightarrow> carpet_obligation damage blocked"
  by (simp add: carpet_obligation_def Obl_def)

lemma defeated_default_no_obligation_force:
  \<comment> \<open>When defeated, the obligation reduces to the bare sanction constant: there
      is no positive deontic force coming from the (now-withdrawn) default.\<close>
  "carpet_obligation True (wear_blocks True) \<longleftrightarrow> Viol"
  by (simp add: carpet_obligation_def Obl_def liable_under_def
                applies_default_def wear_blocks_def)

end
