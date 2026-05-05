import React from 'react';
import './ErrorCard.css';


interface ErrorCardProps {
  err_string: string;
}

const ErrorCard: React.FC<ErrorCardProps> = ({ err_string }) => {
  return (
    <div id="err_card">
      <p id="err_header">ERROR: </p>
      <p id="error_message">{err_string}</p>
    </div>
  );
};

export default ErrorCard;