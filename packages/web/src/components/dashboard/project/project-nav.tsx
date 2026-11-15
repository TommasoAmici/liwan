import styles from "./project-nav.module.css";

import { appPath } from "@/config";
import { cls } from "@/utils";

export type ProjectScreen = "overview" | "utm-tags";

/** The shared project navigation row: Overview and UTM tags. */
export const ProjectNav = ({ projectId, current }: { projectId: string; current: ProjectScreen }) => {
	const links: { screen: ProjectScreen; label: string; href: string }[] = [
		{ screen: "overview", label: "Overview", href: appPath(`/p/${projectId}`) },
		{ screen: "utm-tags", label: "UTM tags", href: appPath(`/p/${projectId}/utm-tags`) },
	];

	return (
		<nav className={styles.nav} aria-label="Project">
			{links.map((link) => (
				<a
					key={link.screen}
					href={link.href}
					className={cls(styles.link, current === link.screen && styles.current)}
					aria-current={current === link.screen ? "page" : undefined}
				>
					{link.label}
				</a>
			))}
		</nav>
	);
};
