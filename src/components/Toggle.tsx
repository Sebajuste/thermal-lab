interface Props {
  checked: boolean;
  disabled?: boolean;
  busy?: boolean;
  /** Ce que l'interrupteur fait, ou pourquoi il est hors service. */
  title: string;
  onChange: () => void;
}

/**
 * Interrupteur à bascule. `role="switch"` plutôt qu'une case à cocher : l'action est
 * immédiate et porte sur un état du système, pas sur un formulaire à valider.
 */
export default function Toggle({ checked, disabled, busy, title, onChange }: Props) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={title}
      title={title}
      className={`toggle ${checked ? "on" : "off"} ${busy ? "busy" : ""}`}
      disabled={disabled || busy}
      onClick={onChange}
    >
      <span className="knob" />
    </button>
  );
}
