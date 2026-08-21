import { SVGProps } from "react";

export const D1Icon = (props: SVGProps<SVGSVGElement>) => (
	<svg viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" {...props}>
		<rect x="3" y="4" width="18" height="16" rx="3" fill="#F6821F" />
		<path
			d="M8 8h8M8 12h8M8 16h5"
			stroke="white"
			strokeWidth="1.6"
			strokeLinecap="round"
		/>
	</svg>
);
