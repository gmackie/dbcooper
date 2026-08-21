import { SVGProps } from "react";

export const TursoIcon = (props: SVGProps<SVGSVGElement>) => (
	<svg viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" {...props}>
		<circle cx="12" cy="12" r="9" fill="#4ADE80" />
		<path
			d="M8 12c0-2.2 1.8-4 4-4s4 1.8 4 4-1.8 4-4 4"
			stroke="#14532d"
			strokeWidth="1.6"
			strokeLinecap="round"
		/>
		<circle cx="12" cy="12" r="1.4" fill="#14532d" />
	</svg>
);
