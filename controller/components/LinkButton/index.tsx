import Link from "next/link";
import React from "react";
import { formatPageHref } from "../../pages/api/util/clientutil";
import styled from "styled-components";

export const Button = styled.button`
  font-size: ${(props) => props.theme.buttonFontSize};
  width: ${(props) => props.theme.buttonWidth};
  height: ${(props) => props.theme.buttonHeight};
  text-align: center;
  margin: ${(props) => props.theme.buttonMargin};
`;

export interface LinkButtonTheme {
  buttonFontSize?: string;
  buttonWidth?: string;
  buttonHeight?: string;
  buttonMargin?: string;
}

export const defaultButtonTheme: LinkButtonTheme = { buttonFontSize: ".8rem", buttonWidth: "fit-content", buttonHeight: "fit-content", buttonMargin: "0 auto" };

// What this returns or calls from the parents
export interface LinkButtonProps {
  name?: string;
  href: string;
  title?: string;
  onClick?: (event: React.MouseEvent<HTMLButtonElement>) => void;
  theme?: LinkButtonTheme;
  children: any;
}

export const LinkButton = ({
  name,
  href,
  title,
  onClick,
  theme,
  children
}: LinkButtonProps) => {
  return (
    <React.Fragment>
      {/* https://nextjs.org/docs/messages/invalid-new-link-with-extra-anchor */}
      <Link href={href} as={formatPageHref(href)} title={title} legacyBehavior>
        <a href={formatPageHref(href)} title={title}>
          <Button name={name} theme={{...defaultButtonTheme, ...theme}} onClick={onClick}>{children}</Button>
        </a>
      </Link>
    </React.Fragment>
  );
};

export default LinkButton;
