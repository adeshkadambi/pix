/** The Pix sparkle mark from the designs. */
export function Logo(props: { size?: number }) {
  const size = () => props.size ?? 26;
  return (
    <svg width={size()} height={size()} viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M12 2C12.8 7.5 16.5 11.2 22 12C16.5 12.8 12.8 16.5 12 22C11.2 16.5 7.5 12.8 2 12C7.5 11.2 11.2 7.5 12 2Z"
        fill="var(--accent)"
      />
      <circle cx="19.5" cy="4.5" r="1.7" fill="var(--violet)" />
    </svg>
  );
}
