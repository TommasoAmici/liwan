import styles from "./help-tip.module.css";

import { Tooltip } from "@base-ui/react";
import { CircleHelpIcon } from "lucide-react";

export const HelpTip = ({ label, description }: { label: string; description: string }) => (
	<Tooltip.Root>
		<Tooltip.Trigger
			type="button"
			className={styles.trigger}
			aria-label={`About ${label}`}
			delay={200}
			closeOnClick={false}
		>
			<CircleHelpIcon size={14} aria-hidden="true" />
		</Tooltip.Trigger>
		<Tooltip.Portal>
			<Tooltip.Positioner side="top" sideOffset={6} className={styles.positioner}>
				<Tooltip.Popup className={styles.popup}>
					<span className={styles.heading}>{label}</span>
					<span className={styles.description}>{description}</span>
				</Tooltip.Popup>
			</Tooltip.Positioner>
		</Tooltip.Portal>
	</Tooltip.Root>
);
